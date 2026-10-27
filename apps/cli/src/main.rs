// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! CLI sin privilegios de red: claves, emisión de configuraciones firmadas y control del agente por su socket Unix.

#[path = "../../agent/src/build_info.rs"]
mod build_info;
mod enroll;
mod ipc;
mod issue;
mod ui;

#[cfg(test)]
#[path = "../../agent/src/platform/private_files.rs"]
pub(crate) mod private_files_contract;
#[cfg(test)]
mod platform {
    pub(crate) use crate::private_files_contract as private_files;
}

/// El contrato del agente (`enroll.rs`, pruebas) referencia `crate::relay` solo para validar una CA.
#[cfg(test)]
mod relay {
    pub fn load_roots(path: Option<&std::path::Path>) -> Result<(), String> {
        path.map_or(Ok(()), |_| Err("sin CA en el contrato".into()))
    }
}

use clap::{Parser, Subcommand};
#[cfg(any(unix, windows))]
use std::io::Write;
#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
#[cfg(any(unix, windows))]
use std::process::{Command, Stdio};
#[cfg(any(unix, windows))]
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
        // Los tokens base64url pueden empezar por '-': no son una opción de clap.
        #[arg(long, allow_hyphen_values = true)]
        token: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        name: String,
        /// Solo laboratorio: acepta `http://` hacia un host que no sea loopback.
        #[arg(long)]
        insecure_http: bool,
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
    /// Estado del agente (JSON); admite el socket de control o el de solo lectura.
    Status {
        #[arg(long)]
        socket: PathBuf,
    },
    /// Para el agente por el socket de control; la interfaz y sus rutas desaparecen con él.
    Down {
        #[arg(long)]
        socket: PathBuf,
    },
    /// Recarga la configuración firmada por el socket de control.
    Reload {
        #[arg(long)]
        socket: PathBuf,
    },
    /// Panel local de solo lectura (navegador) con el estado del agente. Usa el socket de solo lectura.
    Ui {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
}

fn main() -> ExitCode {
    if build_info::print_version("atenea") {
        return ExitCode::SUCCESS;
    }
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
            insecure_http,
        } => enroll::enroll(&url, &token, &dir, &name, insecure_http),
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
        Cmd::Reload { socket } => {
            ask(&socket, "reload")?;
            Ok(())
        }
        Cmd::Ui { socket, port } => ui::serve(&socket, port),
    }
}

#[cfg(unix)]
fn ask(socket: &Path, request: &str) -> Result<String, String> {
    let mut s = ipc::LocalIpc::connect(socket)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    writeln!(s, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let reply: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| "respuesta IPC vacía o JSON inválido")?;
    if let Some(error) = reply.get("error") {
        return Err(format!(
            "agente: {}",
            error.as_str().unwrap_or("orden rechazada")
        ));
    }
    Ok(line.trim().to_string())
}

#[cfg(windows)]
fn ask(socket: &Path, request: &str) -> Result<String, String> {
    let mut stream = ipc::LocalIpc::connect(socket)?;
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let line =
        atenea_windows::pipe_reply(stream, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let reply: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| "respuesta IPC vacía o JSON inválido")?;
    if let Some(error) = reply.get("error") {
        return Err(format!(
            "agente: {}",
            error.as_str().unwrap_or("orden rechazada")
        ));
    }
    Ok(line)
}

#[cfg(not(any(unix, windows)))]
fn ask(_socket: &Path, _request: &str) -> Result<String, String> {
    Err(ipc::LocalIpc::unsupported())
}

#[cfg(not(any(unix, windows)))]
fn up(_config: &Path, _agent: Option<PathBuf>, _log: &Path) -> Result<(), String> {
    Err("arranque local no soportado: backend de servicio e IPC pendiente".into())
}

#[cfg(any(unix, windows))]
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
            .with_file_name(if cfg!(windows) {
                "atenea-agent.exe"
            } else {
                "atenea-agent"
            }),
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
        if ipc::LocalIpc::connect(&socket).is_ok() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(unix, windows)))]
    #[test]
    fn unsupported_local_backend_fails_explicitly() {
        let path = Path::new("unused");
        assert!(ask(path, "status").unwrap_err().contains("pendiente"));
        assert!(up(path, None, path).unwrap_err().contains("pendiente"));
    }

    #[test]
    fn enroll_accepts_tokens_starting_with_hyphen() {
        let cli = Cli::try_parse_from([
            "atenea",
            "enroll",
            "--url",
            "https://x.test",
            "--token",
            "-abc_DEF",
            "--dir",
            "/d",
            "--name",
            "n",
        ])
        .unwrap();
        assert!(matches!(cli.cmd, Cmd::Enroll { token, .. } if token == "-abc_DEF"));
    }

    #[test]
    fn parses_route_and_status_options() {
        assert!(
            Cli::try_parse_from([
                "atenea",
                "agent-config",
                "--join",
                "join.json",
                "--dir",
                "/device",
                "--out",
                "/config.toml",
                "--advertise-route",
                "10.0.0.0/8",
                "--advertise-route",
                "fd12:3456::/32",
                "--status-socket",
                "/status",
                "--status-gid",
                "42",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "atenea",
                "agent-config",
                "--join",
                "join.json",
                "--dir",
                "/device",
                "--out",
                "/config.toml",
                "--status-gid",
                "42",
            ])
            .is_err()
        );
        assert!(matches!(
            Cli::try_parse_from(["atenea", "reload", "--socket", "/control"])
                .unwrap()
                .cmd,
            Cmd::Reload { .. }
        ));
    }

    #[test]
    fn cookie_threshold_cli_rejects_non_positive_or_out_of_range_values() {
        for value in ["1", "100", "4294967295", "0", "-1", "4294967296", "1.5"] {
            let result = Cli::try_parse_from([
                "atenea",
                "agent-config",
                "--join",
                "join.json",
                "--dir",
                "/device",
                "--out",
                "/config.toml",
                "--handshake-cookie-threshold-per-second",
                value,
            ]);
            assert_eq!(result.is_ok(), matches!(value, "1" | "100" | "4294967295"));
        }
        assert!(
            Cli::try_parse_from([
                "atenea",
                "agent-config",
                "--join",
                "join.json",
                "--dir",
                "/device",
                "--out",
                "/config.toml",
            ])
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn status_uses_either_socket_and_control_errors_propagate() {
        use std::os::unix::net::UnixListener;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            Path::new("/tmp").join(format!("atenea-cli-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        for (name, request, response, succeeds) in [
            ("control", "status", r#"{"running":true}"#, true),
            ("status", "status", r#"{"running":true}"#, true),
            ("readonly-stop", "stop", r#"{"error":"solo status"}"#, false),
            (
                "readonly-reload",
                "reload",
                r#"{"error":"solo status"}"#,
                false,
            ),
        ] {
            let socket = directory.join(name);
            let listener = UnixListener::bind(&socket).unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut command = String::new();
                BufReader::new(&stream).read_line(&mut command).unwrap();
                assert_eq!(command.trim(), request);
                writeln!(stream, "{response}").unwrap();
            });
            assert_eq!(ask(&socket, request).is_ok(), succeeds);
            server.join().unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
