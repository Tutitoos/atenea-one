//! CLI sin privilegios de red: claves, emisión de configuraciones firmadas y control del agente por su socket Unix.

mod enroll;
mod issue;

/// El contrato del agente (`enroll.rs`, pruebas) referencia `crate::relay` solo para validar una CA.
#[cfg(test)]
mod relay {
    pub fn load_roots(path: Option<&std::path::Path>) -> Result<(), String> {
        path.map_or(Ok(()), |_| Err("sin CA en el contrato".into()))
    }
}

use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
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
    /// Enrola el paquete público; el token no se guarda.
    Enroll {
        #[arg(long)]
        url: String,
        #[arg(long)]
        token: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Certifica una clave de emisor existente para otra red.
    IssuerCert(issue::IssuerCertArgs),
    /// Genera un TOML local con ancla fijada y sincronización.
    AgentConfig(enroll::AgentConfigArgs),
    /// Genera la clave raíz de firma (fichero 0600; no sobrescribe) e imprime su clave pública (trust anchor).
    /// Debe ejecutarse en la estación de administración, nunca en un host expuesto (ADR-005).
    RootInit {
        #[arg(long)]
        out: PathBuf,
    },
    /// Genera la clave del emisor y su certificado firmado por la raíz.
    IssuerInit {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        network_id: String,
        #[arg(long)]
        serial: u64,
        /// Validez del certificado en días (máximo 90).
        #[arg(long, default_value_t = 30)]
        valid_days: u64,
        /// Clave pública (hex) de la raíz siguiente, para rotarla sin cortar el servicio.
        #[arg(long)]
        next_anchor: Option<String>,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Genera las claves del dispositivo y su paquete público (no contiene secretos).
    DeviceInit {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Emite el fichero de configuración firmado de un dispositivo.
    Issue {
        #[arg(long)]
        issuer_key: PathBuf,
        #[arg(long)]
        issuer_cert: PathBuf,
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
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
        Cmd::Enroll {
            url,
            token,
            dir,
            name,
        } => enroll::enroll(&url, &token, &dir, &name),
        Cmd::IssuerCert(args) => issue::issuer_cert(args),
        Cmd::AgentConfig(args) => enroll::agent_config(args),
        Cmd::RootInit { out } => issue::root_init(&out),
        Cmd::IssuerInit {
            root,
            network_id,
            serial,
            valid_days,
            next_anchor,
            out_dir,
        } => issue::issuer_init(
            &root,
            &network_id,
            serial,
            valid_days,
            next_anchor.as_deref(),
            &out_dir,
        ),
        Cmd::DeviceInit { dir } => issue::device_init(&dir),
        Cmd::Issue {
            issuer_key,
            issuer_cert,
            spec,
            out,
        } => issue::issue(&issuer_key, &issuer_cert, &spec, &out),
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
