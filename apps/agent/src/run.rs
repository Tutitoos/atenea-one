//! Bucle de E/S: TUN, UDP, temporizador e IPC. Toda la lógica vive en `engine`.

use crate::config::{Config, load_key};
use crate::engine::{Engine, Out};
use atenea_tunnel::Tun;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UdpSocket, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};

#[derive(Parser)]
#[command(name = "atenea-agent", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ejecuta el agente en primer plano (requiere CAP_NET_ADMIN).
    Run {
        #[arg(short, long)]
        config: PathBuf,
    },
}

pub fn main() -> ExitCode {
    tracing_subscriber::fmt().with_max_level(log_level()).init();
    let Cmd::Run { config } = Cli::parse().cmd;
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Nivel desde `ATENEA_LOG` (error|warn|info|debug); por defecto info.
fn log_level() -> tracing::Level {
    match std::env::var("ATENEA_LOG").as_deref() {
        Ok("error") => tracing::Level::ERROR,
        Ok("warn") => tracing::Level::WARN,
        Ok("debug") => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

async fn run(path: PathBuf) -> Result<(), String> {
    let cfg = Config::load(&path)?;
    let keys = load_key(&cfg.private_key_file)?;
    let tun = Tun::create(&cfg.interface).map_err(|e| format!("TUN {}: {e}", cfg.interface))?;
    let addrs: Vec<_> = cfg
        .addresses
        .iter()
        .map(|a| (*a, cfg.prefix_for(a).expect("validado en Config")))
        .collect();
    tun.configure(cfg.mtu, &addrs).map_err(|e| e.to_string())?;
    let udp = UdpSocket::bind(cfg.listen)
        .await
        .map_err(|e| format!("UDP {}: {e}", cfg.listen))?;
    let ipc = bind_ipc(&cfg.ipc_socket)?;

    let mut engine = Engine::new(&cfg, keys, Instant::now());
    tracing::info!(interface = %tun.name(), listen = %cfg.listen, peers = cfg.peers.len(), "agente en marcha");

    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut tun_buf = vec![0u8; 4096];
    let mut udp_buf = vec![0u8; 4096];

    loop {
        let outs = tokio::select! {
            r = tun.read_packet(&mut tun_buf) => match r {
                Ok(n) => engine.on_tun(&tun_buf[..n], Instant::now(), unix_now()),
                Err(e) => return Err(format!("lectura TUN: {e}")),
            },
            r = udp.recv_from(&mut udp_buf) => match r {
                Ok((n, from)) => engine.on_udp(from, &udp_buf[..n], Instant::now(), unix_now()),
                Err(e) => { tracing::debug!("recv_from: {e}"); continue }
            },
            _ = tick.tick() => engine.on_tick(Instant::now(), unix_now()),
            r = ipc.accept() => {
                if let Ok((stream, _)) = r && handle_ipc(stream, &engine).await { break; }
                continue;
            }
            _ = term.recv() => break,
            _ = int.recv() => break,
        };
        for o in outs {
            match o {
                Out::Udp(to, dg) => {
                    if let Err(e) = udp.send_to(&dg, to).await {
                        tracing::debug!("send_to {to}: {e}");
                    }
                }
                Out::Tun(p) => {
                    if let Err(e) = tun.write_packet(&p).await {
                        tracing::debug!("escritura TUN: {e}");
                    }
                }
            }
        }
    }
    tracing::info!("cerrando: la interfaz y sus rutas se eliminan al cerrar la TUN");
    let _ = std::fs::remove_file(&cfg.ipc_socket);
    Ok(())
}

fn bind_ipc(path: &std::path::Path) -> Result<UnixListener, String> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        // Un socket vivo significa otro agente con la misma configuración.
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!("{}: ya hay un agente activo", path.display()));
        }
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let l = UnixListener::bind(path).map_err(|e| format!("IPC {}: {e}", path.display()))?;
    // Solo el propietario (root); el reparto por usuario/grupo llega con la fase 7.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    Ok(l)
}

/// Una petición por línea (`status` | `stop`) y una respuesta JSON por línea. `true` = parar.
async fn handle_ipc(stream: UnixStream, engine: &Engine) -> bool {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line)).await;
    if !matches!(read, Ok(Ok(n)) if n > 0) {
        return false;
    }
    let (reply, stop) = match line.trim() {
        "status" => (
            serde_json::to_string(&engine.status(Instant::now())).unwrap_or_default(),
            false,
        ),
        "stop" => (r#"{"ok":true}"#.to_string(), true),
        _ => (r#"{"error":"orden desconocida"}"#.to_string(), false),
    };
    let _ = reader
        .get_mut()
        .write_all(format!("{reply}\n").as_bytes())
        .await;
    stop
}
