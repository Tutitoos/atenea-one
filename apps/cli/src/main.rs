//! CLI sin privilegios de red: genera claves y habla con el agente por su socket Unix.

use atenea_crypto::{StaticKeypair, hex};
use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "atenea", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Genera una clave privada (fichero 0600; no sobrescribe) e imprime la pública.
    Keygen {
        #[arg(long)]
        out: PathBuf,
    },
    /// Imprime la clave pública de un fichero de clave privada.
    Pubkey {
        #[arg(long)]
        key: PathBuf,
    },
    /// Arranca el agente en segundo plano con la configuración dada.
    Up {
        #[arg(short, long)]
        config: PathBuf,
        /// Binario del agente (por defecto, junto a este ejecutable).
        #[arg(long)]
        agent: Option<PathBuf>,
        /// Fichero de registro del agente.
        #[arg(long, default_value = "atenea-agent.log")]
        log: PathBuf,
    },
    /// Estado del agente (JSON).
    Status {
        #[arg(long)]
        socket: PathBuf,
    },
    /// Para el agente; la interfaz y sus rutas desaparecen con él.
    Down {
        #[arg(long)]
        socket: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Cmd) -> Result<(), String> {
    match cmd {
        Cmd::Keygen { out } => {
            let kp = StaticKeypair::generate();
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&out)
                .map_err(|e| format!("{}: {e}", out.display()))?;
            writeln!(f, "{}", hex::encode(kp.private_bytes())).map_err(|e| e.to_string())?;
            println!("{}", hex::encode(&kp.public));
            Ok(())
        }
        Cmd::Pubkey { key } => {
            let text =
                std::fs::read_to_string(&key).map_err(|e| format!("{}: {e}", key.display()))?;
            let private =
                hex::decode::<32>(&text).ok_or("clave privada: se esperan 64 dígitos hex")?;
            println!(
                "{}",
                hex::encode(&StaticKeypair::from_private(private).public)
            );
            Ok(())
        }
        Cmd::Up { config, agent, log } => up(&config, agent, &log),
        Cmd::Status { socket } => {
            println!("{}", ask(&socket, "status")?);
            Ok(())
        }
        Cmd::Down { socket } => {
            ask(&socket, "stop")?;
            Ok(())
        }
    }
}

fn ask(socket: &Path, request: &str) -> Result<String, String> {
    let mut s = UnixStream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    writeln!(s, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

fn up(config: &Path, agent: Option<PathBuf>, log: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    let value: toml::Table =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", config.display()))?;
    let socket = PathBuf::from(
        value
            .get("ipc_socket")
            .and_then(|v| v.as_str())
            .ok_or("la configuración no define ipc_socket")?,
    );
    let agent = match agent {
        Some(a) => a,
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("atenea-agent"),
    };
    let logf = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let mut child = Command::new(&agent)
        .args(["run", "--config"])
        .arg(config)
        .stdin(Stdio::null())
        .stdout(logf.try_clone().map_err(|e| e.to_string())?)
        .stderr(logf)
        .spawn()
        .map_err(|e| format!("{}: {e}", agent.display()))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "el agente terminó ({st}); revisa {}",
                log.display()
            ));
        }
        if UnixStream::connect(&socket).is_ok() {
            println!(
                "agente en marcha (pid {}); socket {}",
                child.id(),
                socket.display()
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "el agente no respondió en 5 s; revisa {}",
        log.display()
    ))
}
