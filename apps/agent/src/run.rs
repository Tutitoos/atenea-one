//! Bucle de E/S: TUN, UDP, temporizador, recarga de configuración firmada e IPC.
//! Toda la lógica de protocolo vive en `engine`; la verificación, en `atenea-identity`.

use crate::config::{Config, load_identity_key, load_noise_key};
use crate::engine::{Engine, Out};
use crate::state;
use atenea_identity::{AgentState, LocalKeys, MAX_BODY_LEN, verify_config};
use atenea_policies::prefix_contains;
use atenea_tunnel::Tun;
use clap::{Parser, Subcommand};
use std::net::IpAddr;
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

/// Estado de ejecución que cambia con cada configuración aplicada.
struct Runtime {
    cfg: Config,
    local: LocalKeys,
    state: AgentState,
    tun: Tun,
    engine: Engine,
    /// (mtime, longitud) del último fichero de configuración procesado, para no reprocesarlo.
    seen: Option<(SystemTime, u64)>,
    /// MTU y direcciones ya instaladas en la interfaz.
    iface: Option<(usize, Vec<(IpAddr, u8)>)>,
    last_saved: u64,
}

impl Runtime {
    /// Lee, verifica, persiste y aplica la configuración firmada si el fichero cambió.
    /// Un rechazo no revoca la vigente: sigue hasta que caduque su lease.
    fn poll_config(&mut self, unix: u64) {
        let Ok(meta) = std::fs::metadata(&self.cfg.config_file) else {
            return;
        };
        let key = (meta.modified().unwrap_or(UNIX_EPOCH), meta.len());
        if self.seen == Some(key) {
            return;
        }
        self.seen = Some(key);
        if meta.len() as usize > MAX_BODY_LEN + 512 {
            self.reject("too_large");
            return;
        }
        let bytes = match std::fs::read(&self.cfg.config_file) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("configuración ilegible: {e}");
                self.seen = None; // reintentar en el siguiente ciclo
                return;
            }
        };
        let v = match verify_config(&bytes, &self.local, &self.state, unix) {
            Ok(v) => v,
            Err(e) => {
                self.reject(e.code());
                return;
            }
        };
        // Se persiste ANTES de aplicar: un fallo no aplica nada y conserva la configuración anterior.
        if !v.noop || v.state != self.state {
            if let Err(e) = state::save(&self.cfg.state_dir, &v.state) {
                tracing::error!("no se pudo persistir el estado: {e}");
                self.reject("state_persist_failed");
                return;
            }
            self.last_saved = unix;
        }
        let addrs: Vec<(IpAddr, u8)> = v
            .config
            .tunnel_addrs
            .iter()
            .filter_map(|a| {
                v.config
                    .network_prefixes
                    .iter()
                    .find(|p| prefix_contains(p, a))
                    .map(|p| (*a, p.len))
            })
            .collect();
        let wanted = (usize::from(v.config.mtu), addrs);
        if self.iface.as_ref() != Some(&wanted) {
            if let Err(e) = self.tun.configure(wanted.0, &wanted.1) {
                tracing::error!("configurando la interfaz: {e}");
                self.reject("interface_failed");
                return;
            }
            self.iface = Some(wanted);
        }
        self.engine.apply_config(&v.config);
        tracing::info!(
            version = v.config.config_version,
            noop = v.noop,
            "configuración aplicada"
        );
        self.state = v.state;
    }

    fn reject(&mut self, code: &'static str) {
        tracing::warn!(code, "configuración rechazada");
        self.engine.note_config_error(code);
    }

    /// Mantiene la hora máxima observada (anti-rollback de reloj) y la persiste como mucho cada 60 s.
    fn housekeeping(&mut self, unix: u64) {
        self.state.max_time_observed = self.state.max_time_observed.max(unix);
        if unix.saturating_sub(self.last_saved) >= 60 {
            if let Err(e) = state::save(&self.cfg.state_dir, &self.state) {
                tracing::error!("no se pudo persistir el estado: {e}");
            }
            self.last_saved = unix;
        }
    }
}

async fn run(path: PathBuf) -> Result<(), String> {
    let cfg = Config::load(&path)?;
    let noise = load_noise_key(&cfg.noise_key_file)?;
    let identity = load_identity_key(&cfg.identity_key_file)?;
    let local = LocalKeys {
        noise_pub: noise.public,
        identity_pub: identity.verifying_key().to_bytes(),
    };
    let state = match state::load(&cfg.state_dir)? {
        Some(s) if s.network_id == cfg.network_id && s.device_id == cfg.device_id => s,
        Some(_) => return Err("el estado persistido pertenece a otra red o dispositivo".into()),
        None => {
            let s = AgentState::new(cfg.network_id, cfg.device_id, cfg.trust_anchor);
            state::save(&cfg.state_dir, &s).map_err(|e| format!("estado inicial: {e}"))?;
            s
        }
    };
    let tun = Tun::create(&cfg.interface).map_err(|e| format!("TUN {}: {e}", cfg.interface))?;
    let udp = UdpSocket::bind(cfg.listen)
        .await
        .map_err(|e| format!("UDP {}: {e}", cfg.listen))?;
    let ipc = bind_ipc(&cfg.ipc_socket)?;
    let engine = Engine::new(cfg.network_id, noise, Instant::now());
    let mut rt = Runtime {
        cfg,
        local,
        state,
        tun,
        engine,
        seen: None,
        iface: None,
        last_saved: unix_now(),
    };
    tracing::info!(interface = %rt.tun.name(), listen = %rt.cfg.listen, "agente en marcha; sin tráfico hasta cargar una configuración vigente");
    rt.poll_config(unix_now());

    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut tun_buf = vec![0u8; 4096];
    let mut udp_buf = vec![0u8; 4096];

    loop {
        let outs = tokio::select! {
            r = rt.tun.read_packet(&mut tun_buf) => match r {
                Ok(n) => rt.engine.on_tun(&tun_buf[..n], Instant::now(), unix_now()),
                Err(e) => return Err(format!("lectura TUN: {e}")),
            },
            r = udp.recv_from(&mut udp_buf) => match r {
                Ok((n, from)) => rt.engine.on_udp(from, &udp_buf[..n], Instant::now(), unix_now()),
                Err(e) => { tracing::debug!("recv_from: {e}"); continue }
            },
            _ = tick.tick() => {
                let unix = unix_now();
                rt.poll_config(unix);
                rt.housekeeping(unix);
                rt.engine.on_tick(Instant::now(), unix)
            },
            r = ipc.accept() => {
                if let Ok((stream, _)) = r && handle_ipc(stream, &rt.engine).await { break; }
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
                    if let Err(e) = rt.tun.write_packet(&p).await {
                        tracing::debug!("escritura TUN: {e}");
                    }
                }
            }
        }
    }
    tracing::info!("cerrando: la interfaz y sus rutas se eliminan al cerrar la TUN");
    let _ = state::save(&rt.cfg.state_dir, &rt.state);
    let _ = std::fs::remove_file(&rt.cfg.ipc_socket);
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
            serde_json::to_string(&engine.status(Instant::now(), unix_now())).unwrap_or_default(),
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
