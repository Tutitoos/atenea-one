//! Bucle de E/S: TUN, UDP, temporizador, recarga de configuración firmada e IPC.
//! Toda la lógica de protocolo vive en `engine`; la verificación, en `atenea-identity`.

use crate::config::{Config, load_identity_key, load_noise_key};
use crate::engine::{Engine, NatStatus, Out};
use crate::net::{self, Stun, from_wire, to_wire};
use crate::platform::{HostNetwork, Lifecycle, Platform, TunDevice};
use crate::relay::{RelayClient, RelayClientConfig, RelayEvent};
use crate::state;
use atenea_identity::{AgentState, LocalKeys, MAX_BODY_LEN, verify_config};
use atenea_nat_traversal::looks_like_stun;
use atenea_policies::prefix_contains;
use clap::{Parser, Subcommand};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::Notify;

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
        /// Se conecta al Service Control Manager de Windows.
        #[arg(long, hide = !cfg!(target_os = "windows"))]
        service: bool,
    },
    /// Elimina exclusivamente rutas y DNS propiedad del agente.
    Cleanup {
        #[arg(short, long)]
        config: PathBuf,
    },
    /// Con el agente parado: borra solo la hora máxima observada (tras un reloj mal ajustado).
    /// Conserva versión, serial y anclas del estado anti-rollback.
    ResetClock {
        #[arg(short, long)]
        config: PathBuf,
    },
}

pub fn main() -> ExitCode {
    let cmd = Cli::parse().cmd;
    #[cfg(target_os = "windows")]
    if matches!(&cmd, Cmd::Run { service: true, .. }) {
        if let Err(error) = crate::platform::service::init_logging(log_level()) {
            eprintln!("diagnóstico del servicio: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        tracing_subscriber::fmt().with_max_level(log_level()).init();
    }
    #[cfg(not(target_os = "windows"))]
    tracing_subscriber::fmt().with_max_level(log_level()).init();
    let config = match cmd {
        Cmd::Run { config, service } => {
            #[cfg(target_os = "windows")]
            if service {
                return match crate::platform::service::run_service(config) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        tracing::error!("{error}");
                        ExitCode::FAILURE
                    }
                };
            }
            #[cfg(not(target_os = "windows"))]
            if service {
                tracing::error!("--service sólo está disponible en Windows");
                return ExitCode::FAILURE;
            }
            config
        }
        Cmd::Cleanup { config } => {
            return match Config::load(&config).and_then(|cfg| {
                let mut netconf = crate::netconf::NetConf::new(cfg.interface, Default::default());
                netconf.cleanup()?;
                #[cfg(target_os = "windows")]
                netconf.remove_orphan_adapter()?;
                Ok(())
            }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
        Cmd::ResetClock { config } => {
            return match Config::load(&config).and_then(|cfg| state::reset_clock(&cfg.state_dir)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
    };
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
    let result = rt.block_on(run(config));
    shutdown_runtime(rt);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Solo después de que `run` termine y libere TUN/red/IPC. Abortar una tarea
/// async no cancela su HTTP síncrono: no esperar indefinidamente a esos hilos.
pub(crate) fn shutdown_runtime(runtime: tokio::runtime::Runtime) {
    runtime.shutdown_timeout(Duration::from_secs(2));
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
    store: state::Store,
    /// Fin del lease de la última configuración verificada: tope de la hora observada.
    horizon: Option<u64>,
    tun: TunDevice,
    engine: Engine,
    /// (mtime, longitud) del último fichero de configuración procesado, para no reprocesarlo.
    seen: Option<(SystemTime, u64)>,
    /// MTU y direcciones ya instaladas en la interfaz.
    iface: Option<(usize, Vec<(IpAddr, u8)>)>,
    netconf: crate::netconf::NetConf,
    resolved_endpoints: Option<Vec<IpAddr>>,
    routes_changed: bool,
    candidate_exclusions: Vec<atenea_identity::Prefix>,
    /// Red del túnel, rutas de los pares y subredes servidas: no son destinos de relay.
    tunnel_only: Vec<atenea_identity::Prefix>,
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
        let v = match verify_config(&bytes, &self.local, self.store.state(), unix) {
            Ok(v) => v,
            Err(e) => {
                self.reject(e.code());
                return;
            }
        };
        // Se persiste ANTES de aplicar y el estado en memoria cambia a la vez (`commit`): un fallo
        // posterior no aplica nada, conserva la configuración anterior y no hace retroceder el estado.
        if v.state != *self.store.state()
            && let Err(e) = self.store.commit(v.state, unix)
        {
            tracing::error!("no se pudo persistir el estado: {e}");
            self.reject("state_persist_failed");
            return;
        }
        self.horizon = Some(v.config.not_after);
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
            if let Err(e) = self.netconf.configure_link(wanted.0, &wanted.1) {
                tracing::error!("configurando la interfaz: {e}");
                self.reject("interface_failed");
                self.seen = None;
                return;
            }
            self.iface = Some(wanted);
        }
        self.netconf
            .endpoints_resolved(self.resolved_endpoints.is_some());
        let protected = self.resolved_endpoints.clone().unwrap_or_default();
        if let Err(e) = self.netconf.apply(&v.config, &protected) {
            tracing::error!("configurando la red: {e}");
            self.reject("network_failed");
            self.seen = None;
            return;
        }
        self.routes_changed = true;
        self.candidate_exclusions = v
            .config
            .network_prefixes
            .iter()
            .cloned()
            .chain(v.config.advertised.iter().map(|r| r.prefix.clone()))
            .collect();
        self.tunnel_only = self
            .candidate_exclusions
            .iter()
            .cloned()
            .chain(v.config.peers.iter().flat_map(|p| p.routes.iter().cloned()))
            .collect();
        self.engine.apply_config(&v.config);
        tracing::info!(
            version = v.config.config_version,
            noop = v.noop,
            "configuración aplicada"
        );
    }

    fn reject(&mut self, code: &'static str) {
        tracing::warn!(code, "configuración rechazada");
        self.engine.note_config_error(code);
    }

    /// Mantiene la hora máxima observada (anti-rollback de reloj), acotada por el lease validado.
    fn housekeeping(&mut self, unix: u64) {
        self.store.tick(unix, self.horizon);
    }
}

pub(crate) async fn run(path: PathBuf) -> Result<(), String> {
    let cfg = Config::load(&path)?;
    if !cfg.advertise_routes.is_empty()
        && !Platform::new(cfg.interface.clone(), Default::default()).supports_subnet_routing()
    {
        return Err(
            "advertise_routes no soportado: esta plataforma no admite servir subredes".into(),
        );
    }
    Platform::ensure_available()?;
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
    let store = state::Store::new(cfg.state_dir.clone(), state, unix_now());
    let tun = TunDevice::open(&cfg.interface).map_err(|e| format!("TUN {}: {e}", cfg.interface))?;
    let udp = UdpSocket::bind(cfg.listen)
        .await
        .map_err(|e| format!("UDP {}: {e}", cfg.listen))?;
    let ipc = crate::ipc::bind(&cfg.ipc_socket, None, false)?;
    let status_ipc = cfg
        .status_socket
        .as_ref()
        .map(|p| crate::ipc::bind(p, cfg.status_gid, true))
        .transpose()?;
    let mut netconf = crate::netconf::NetConf::new(tun.name().into(), Default::default());
    netconf.cleanup()?;
    let resolved_endpoints = if cfg.sync.is_none() && cfg.coordinator.is_none() {
        Some(Vec::new())
    } else {
        None
    };
    netconf.endpoints_resolved(resolved_endpoints.is_some());
    let (sync_tx, sync_rx) =
        tokio::sync::watch::channel(cfg.sync.as_ref().map(|s| crate::sync::Status {
            url: s.url.clone(),
            ..Default::default()
        }));
    let report_tx = sync_tx.clone();
    let sync_task = cfg.sync.clone().map(|sync| {
        let path = cfg.config_file.clone();
        let device = cfg.device_id;
        let identity = identity.clone();
        let snapshots = store.subscribe();
        let (noise_pub, identity_pub) = (local.noise_pub, local.identity_pub);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(sync.interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let sync = sync.clone();
                let identity = identity.clone();
                let path = path.clone();
                let snapshot = snapshots.borrow().clone();
                let result = tokio::task::spawn_blocking(move || {
                    // Solo una configuración que verifica contra el estado vigente sustituye al fichero.
                    let keys = LocalKeys {
                        noise_pub,
                        identity_pub,
                    };
                    let verify = |body: &[u8]| {
                        verify_config(body, &keys, &snapshot, unix_now())
                            .map(|_| ())
                            .map_err(|e| e.code())
                    };
                    crate::sync::fetch(&sync, &identity, device, &path, unix_now(), &verify)
                })
                .await
                .unwrap_or(Err("fallo de la tarea de sincronización"));
                sync_tx.send_modify(|s| {
                    if let Some(s) = s {
                        s.record(result, unix_now());
                    }
                });
            }
        })
    });
    let mut engine = Engine::new(cfg.network_id, noise, unix_now());
    engine.set_cookie_threshold(cfg.handshake_cookie_threshold_per_second);
    let dual = cfg.listen.is_ipv6();
    let device = cfg.device_id;

    // Coordinator (fase 4): anuncio de presencia, avisos de pares y descubrimiento de candidatos.
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<CoordMsg>(64);
    let vet_slots = Arc::new(tokio::sync::Semaphore::new(4));
    let (cand_tx, cand_rx) = tokio::sync::watch::channel::<Vec<SocketAddr>>(Vec::new());
    let announce_now = Arc::new(Notify::new());
    let (coord_tx, coord_rx) =
        tokio::sync::watch::channel(cfg.coordinator.as_ref().map(|c| CoordStatus {
            url: c.url.clone(),
            ..Default::default()
        }));
    let mut coord_tasks = Vec::new();
    if let Some(c) = cfg.coordinator.clone() {
        let (c1, id1, path1, rx1, n1, tx1) = (
            c.clone(),
            identity.clone(),
            cfg.config_file.clone(),
            cand_rx.clone(),
            announce_now.clone(),
            coord_tx.clone(),
        );
        coord_tasks.push(tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(20));
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! { _ = every.tick() => {}, _ = n1.notified() => {} }
                let Ok(bytes) = std::fs::read(&path1) else {
                    continue;
                };
                if !bytes.starts_with(b"AOCF") {
                    continue;
                }
                let cands = rx1.borrow().clone();
                let (c, k) = (c1.clone(), id1.clone());
                let res = tokio::task::spawn_blocking(move || {
                    crate::coord::announce(&c, &k, device, &bytes, &cands, unix_now())
                })
                .await
                .unwrap_or(Err("fallo de la tarea"));
                tx1.send_modify(|s| {
                    if let Some(s) = s {
                        s.announces += 1;
                        match res {
                            Ok(()) => {
                                s.last_announce_ok_unix = Some(unix_now());
                                s.last_error = None;
                            }
                            Err(e) => s.last_error = Some(e),
                        }
                    }
                });
            }
        }));
        let (c2, id2, tx2, ctx2) = (
            c.clone(),
            identity.clone(),
            msg_tx.clone(),
            coord_tx.clone(),
        );
        coord_tasks.push(tokio::spawn(async move {
            loop {
                let (c, k) = (c2.clone(), id2.clone());
                let res = tokio::task::spawn_blocking(move || {
                    crate::coord::events(&c, &k, device, unix_now(), 25)
                })
                .await
                .unwrap_or(Err("fallo de la tarea"));
                match res {
                    Ok(events) => {
                        for e in events {
                            ctx2.send_modify(|s| {
                                if let Some(s) = s {
                                    s.events += 1;
                                }
                            });
                            match e {
                                crate::coord::Event::Connect { peer, candidates } => {
                                    let _ = tx2.send(CoordMsg::Candidates(peer, candidates)).await;
                                }
                                crate::coord::Event::Relay { peer, grant } => {
                                    let _ = tx2.send(CoordMsg::Relay(peer, grant)).await;
                                }
                            }
                        }
                    }
                    // Aún sin presencia o coordinator caído: reintento con pausa.
                    Err(_) => tokio::time::sleep(Duration::from_secs(3)).await,
                }
            }
        }));
    }
    let relay_on = cfg.coordinator.as_ref().is_some_and(|c| c.relay != "off");
    let (relay, mut relay_rx) = RelayClient::new(
        RelayClientConfig {
            ca_file: cfg
                .coordinator
                .as_ref()
                .and_then(|c| c.relay_ca_file.clone()),
        },
        identity.clone(),
        device,
        cfg.network_id,
    );
    let mut stun = cfg.coordinator.as_ref().map(|c| Stun::new(c.stun.clone()));
    let mut stun_started: Option<Instant> = None;

    let mut rt = Runtime {
        cfg,
        local,
        store,
        horizon: None,
        tun,
        engine,
        seen: None,
        iface: None,
        netconf,
        resolved_endpoints,
        routes_changed: true,
        candidate_exclusions: Vec::new(),
        tunnel_only: Vec::new(),
    };
    tracing::info!(interface = %rt.tun.name(), listen = %rt.cfg.listen, "agente en marcha; sin tráfico hasta cargar una configuración vigente");
    rt.poll_config(unix_now());

    let mut lifecycle = Lifecycle::new().map_err(|e| e.to_string())?;
    // Paso fino (250 ms) para el sondeo con jitter; lo caro (configuración, estado) cada segundo.
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut stun_refresh = tokio::time::interval(Duration::from_secs(30));
    let mut ticks: u64 = 0;
    let mut report_every = tokio::time::interval(Duration::from_secs(15));
    report_every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut report_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut route_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut last_declaration: Option<Instant> = None;
    let mut routes_declared = false;
    let mut endpoint_task: Option<tokio::task::JoinHandle<Result<Vec<IpAddr>, String>>> = None;
    let mut endpoint_started: Option<Instant> = None;
    let mut tun_buf = vec![0u8; 4096];
    let mut udp_buf = vec![0u8; 4096];

    let snapshot = || {
        std::sync::Arc::<str>::from(
            ipc_status(
                &rt,
                sync_rx.borrow().clone(),
                coord_rx.borrow().clone(),
                relay.status(),
            )
            .to_string(),
        )
    };
    let (status_tx, status_rx) = tokio::sync::watch::channel(snapshot());
    let (action_tx, mut action_rx) = tokio::sync::mpsc::channel(16);
    let _ipc_server = crate::ipc::serve(
        ipc,
        rt.cfg.ipc_socket.clone(),
        false,
        status_rx.clone(),
        action_tx.clone(),
    );
    let _status_server = status_ipc
        .zip(rt.cfg.status_socket.clone())
        .map(|(listener, path)| crate::ipc::serve(listener, path, true, status_rx, action_tx));
    loop {
        let outs = tokio::select! {
            r = rt.tun.recv(&mut tun_buf) => match r {
                Ok(n) => rt.engine.on_tun(&tun_buf[..n], Instant::now(), unix_now()),
                Err(e) => rt.engine.on_tun_error(e).map_err(|e| format!("lectura TUN: {e}"))?,
            },
            r = udp.recv_from(&mut udp_buf) => match r {
                Ok((n, from)) => {
                    let data = &udp_buf[..n];
                    if looks_like_stun(data) {
                        // Respuesta STUN en el socket de datos compartido (ver nat-traversal).
                        if let Some(s) = stun.as_mut() {
                            s.on_response(data);
                        }
                        Vec::new()
                    } else {
                        rt.engine.on_udp(from_wire(from), data, Instant::now(), unix_now())
                    }
                }
                Err(e) => { tracing::debug!("recv_from: {e}"); continue }
            },
            _ = tick.tick() => {
                let unix = unix_now();
                let now = Instant::now();
                ticks += 1;
                if ticks.is_multiple_of(4) {
                    status_tx.send_replace(std::sync::Arc::<str>::from(
                        ipc_status(&rt, sync_rx.borrow().clone(), coord_rx.borrow().clone(), relay.status()).to_string()
                    ));
                    if endpoint_task.as_ref().is_some_and(|task| task.is_finished())
                        && let Some(task) = endpoint_task.take()
                    {
                        let resolved = task.await.ok().and_then(Result::ok);
                        if rt.resolved_endpoints != resolved {
                            rt.resolved_endpoints = resolved;
                            rt.seen = None;
                        }
                    }
                    let refresh = endpoint_started.is_none_or(|started| now.duration_since(started) >= Duration::from_secs(10));
                    // Una sola resolución bloqueante en vuelo; no bloquea TUN/IPC ni acumula hilos.
                    if refresh && endpoint_task.is_some() && rt.resolved_endpoints.take().is_some() {
                        rt.seen = None;
                    }
                    if refresh && endpoint_task.is_none() && (rt.cfg.sync.is_some() || rt.cfg.coordinator.is_some()) {
                        let sync = rt.cfg.sync.clone();
                        let coordinator = rt.cfg.coordinator.clone();
                        endpoint_task = Some(tokio::task::spawn_blocking(move || protected_endpoints(sync, coordinator)));
                        endpoint_started = Some(now);
                    }
                    rt.poll_config(unix);
                    let before = cand_tx.borrow().len();
                    cand_tx.send_modify(|cands| net::exclude_tunnel_candidates(cands, &rt.candidate_exclusions));
                    if cand_tx.borrow().len() != before { announce_now.notify_one(); }
                    rt.housekeeping(unix);
                    let elapsed = last_declaration.map_or(Duration::MAX, |last| now.duration_since(last));
                    if (!rt.cfg.advertise_routes.is_empty() || routes_declared)
                        && elapsed >= Duration::from_secs(5)
                        && (rt.routes_changed || elapsed >= Duration::from_secs(300))
                        && route_task.as_ref().is_none_or(|task| task.is_finished())
                        && let Some(sync) = rt.cfg.sync.clone()
                    {
                        let routes = rt.cfg.advertise_routes.clone();
                        let key = identity.clone();
                        let tx = report_tx.clone();
                        routes_declared = true;
                        rt.routes_changed = false;
                        last_declaration = Some(now);
                        route_task = Some(tokio::spawn(async move {
                            let result = tokio::task::spawn_blocking(move || crate::sync::declare_routes(&sync, &key, device, &routes, unix_now())).await;
                            if !matches!(result, Ok(Ok(()))) {
                                tx.send_modify(|s| {
                                    if let Some(s) = s { s.route_announce_errors = s.route_announce_errors.saturating_add(1); }
                                });
                            }
                        }));
                    }
                }
                // STUN: tras la respuesta (o 3 s), recalcula candidatos y NAT y anuncia si cambian.
                if let (Some(s), Some(started)) = (stun.as_mut(), stun_started) {
                    s.expire(now, Duration::from_secs(3));
                    if s.complete() || now.duration_since(started) >= Duration::from_secs(3) {
                        stun_started = None;
                        let (mapping, reflexive) = s.result();
                        let mut cands = net::local_addrs(rt.tun.name(), rt.cfg.listen.port(), &rt.candidate_exclusions).await;
                        if let Some(r) = reflexive && !cands.contains(&r) {
                            cands.push(r);
                        }
                        net::exclude_tunnel_candidates(&mut cands, &rt.candidate_exclusions);
                        rt.engine.set_nat(NatStatus {
                            mapping: match mapping { "independent" => "independent", "dependent" => "dependent", _ => "unknown" },
                            reflexive: reflexive.map(|r| r.to_string()),
                            candidates: cands.iter().map(|c| c.to_string()).collect(),
                        });
                        if *cand_tx.borrow() != cands {
                            tracing::info!(mapping, candidates = cands.len(), "candidatos propios actualizados");
                            let _ = cand_tx.send(cands);
                            announce_now.notify_one();
                        }
                    }
                }
                // Rendezvous pendientes (sin ruta confirmada y con tráfico o ruta perdida).
                if let Some(c) = rt.cfg.coordinator.clone() {
                    for peer in rt.engine.take_rendezvous(now) {
                        let (c, k, tx, st) = (c.clone(), identity.clone(), msg_tx.clone(), coord_tx.clone());
                        tokio::spawn(async move {
                            let res = tokio::task::spawn_blocking(move || {
                                crate::coord::rendezvous(&c, &k, device, peer, unix_now())
                            })
                            .await
                            .unwrap_or(Err("fallo de la tarea"));
                            st.send_modify(|s| {
                                if let Some(s) = s {
                                    s.rendezvous += 1;
                                    if let Err(e) = &res {
                                        s.last_error = Some(e);
                                    }
                                }
                            });
                            if let Ok(cands) = res {
                                let _ = tx.send(CoordMsg::Candidates(peer, cands)).await;
                            }
                        });
                    }
                }
                // Relay: petición cuando el directo no llega; se libera cuando el directo es estable.
                if relay_on && let Some(c) = rt.cfg.coordinator.clone() {
                    for peer in rt.engine.take_relay_requests(now) {
                        let (c, k, tx) = (c.clone(), identity.clone(), msg_tx.clone());
                        tokio::spawn(async move {
                            let res = tokio::task::spawn_blocking(move || {
                                crate::coord::relay(&c, &k, device, peer, unix_now())
                            })
                            .await
                            .unwrap_or(Err("fallo de la tarea"));
                            match res {
                                Ok(g) => {
                                    let _ = tx.send(CoordMsg::Relay(peer, g)).await;
                                }
                                Err(e) => tracing::debug!("relay {}: {e}", atenea_crypto::hex::encode(&peer)),
                            }
                        });
                    }
                    for peer in rt.engine.take_relay_releases(now) {
                        relay.close(peer);
                    }
                }
                rt.engine.on_tick(now, unix)
            },
            _ = report_every.tick(), if rt.cfg.sync.is_some() => {
                if report_task.as_ref().is_none_or(|task| task.is_finished())
                    && let Some(sync) = rt.cfg.sync.clone()
                    && let Some(mut report) = crate::sync::build_report(&rt.engine.status(Instant::now(), unix_now()), &relay.status())
                {
                    report.network(&rt.netconf.status());
                    let key = identity.clone();
                    let tx = report_tx.clone();
                    report_task = Some(tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(move || {
                            crate::sync::send_report(&sync, &key, device, &report, unix_now())
                        })
                        .await
                        .unwrap_or(Err("fallo de la tarea de informe"));
                        if result.is_err() {
                            tx.send_modify(|status| {
                                if let Some(status) = status {
                                    status.report_errors = status.report_errors.saturating_add(1);
                                }
                            });
                        }
                    }));
                }
                Vec::new()
            },
            _ = stun_refresh.tick(), if stun.is_some() => {
                if let Some(s) = stun.as_mut() {
                    s.start(&udp, dual, Instant::now()).await;
                    stun_started = Some(Instant::now());
                }
                Vec::new()
            },
            Some(msg) = msg_rx.recv() => match msg {
                CoordMsg::Candidates(peer, cands) => {
                    rt.engine.on_peer_candidates(&peer, cands, Instant::now(), unix_now())
                }
                CoordMsg::Relay(peer, grant) => {
                    // Las concesiones son pistas de un coordinator no fiable: solo para pares
                    // configurados, con pocas resoluciones simultáneas y sin tocar la red local.
                    if relay_on
                        && rt.engine.knows_peer(&peer)
                        && let Ok(permit) = vet_slots.clone().try_acquire_owned()
                    {
                        let (tx, tunnel_only) = (msg_tx.clone(), rt.tunnel_only.clone());
                        tokio::spawn(async move {
                            let relays = crate::coord::vet_relays(grant.relays, &tunnel_only).await;
                            drop(permit);
                            if !relays.is_empty() {
                                let _ = tx.send(CoordMsg::RelayVetted(peer, relays)).await;
                            }
                        });
                    }
                    Vec::new()
                }
                CoordMsg::RelayVetted(peer, relays) => {
                    if relay_on && rt.engine.knows_peer(&peer) { relay.open(peer, relays); }
                    Vec::new()
                }
            },
            Some(ev) = relay_rx.recv() => {
                let now = Instant::now();
                match ev {
                    RelayEvent::Ready { peer, relay_id, kind } => rt.engine.set_relay_ready(&peer, relay_id, kind, true, now),
                    RelayEvent::Down { peer, relay_id, kind } => rt.engine.set_relay_ready(&peer, relay_id, kind, false, now),
                    RelayEvent::Rx { peer, relay_id, kind, datagram } => {
                        rt.engine.on_relay_rx(&peer, relay_id, kind, &datagram, now, unix_now())
                    }
                    RelayEvent::Rtt { peer, relay_id, kind, ms } => {
                        rt.engine.set_relay_rtt(&peer, relay_id, kind, ms);
                        Vec::new()
                    }
                }
            },
            Some(action) = action_rx.recv() => {
                    match action {
                        crate::ipc::Action::Stop => break,
                        crate::ipc::Action::Reload => {
                            match Config::load(&path) {
                                Ok(cfg) => {
                                    if cfg.advertise_routes.is_empty()
                                        || Platform::new(cfg.interface, Default::default()).supports_subnet_routing()
                                    {
                                        rt.cfg.advertise_routes = cfg.advertise_routes;
                                    } else {
                                        tracing::warn!("advertise_routes rechazado: esta plataforma sólo admite clientes");
                                    }
                                }
                                Err(error) => tracing::warn!("recarga local rechazada: {error}"),
                            }
                            rt.seen = None; rt.routes_changed = true;
                        }
                        crate::ipc::Action::Cleanup => { let _ = rt.netconf.cleanup(); rt.seen = None; }
                        crate::ipc::Action::None => {}
                    }
                continue;
            }
            _ = lifecycle.wait() => {
                tracing::info!("parada recibida por el bucle del agente");
                break;
            },
        };
        for o in outs {
            match o {
                Out::Udp(to, dg) => {
                    let Some(wire) = to_wire(to, dual) else {
                        continue;
                    };
                    if let Err(e) = udp.send_to(&dg, wire).await {
                        tracing::debug!("send_to {to}: {e}");
                    }
                }
                Out::Tun(p) => {
                    if let Err(e) = rt.tun.send(&p).await {
                        tracing::debug!("escritura TUN: {e}");
                    }
                }
                Out::Relay {
                    peer,
                    relay_id,
                    kind,
                    dg,
                } => relay.send(peer, relay_id, kind, dg),
            }
        }
    }
    if let Some(task) = sync_task {
        task.abort();
    }
    if let Some(task) = report_task {
        task.abort();
    }
    if let Some(task) = route_task {
        task.abort();
    }
    for t in coord_tasks {
        t.abort();
    }
    tracing::info!("cerrando: la interfaz y sus rutas se eliminan al cerrar la TUN");
    #[cfg(target_os = "windows")]
    rt.netconf.cleanup()?;
    rt.store.flush();
    let _ = std::fs::remove_file(&rt.cfg.ipc_socket);
    if let Some(path) = &rt.cfg.status_socket {
        let _ = std::fs::remove_file(path);
    }
    drop(rt);
    tracing::info!("limpieza de red terminada");
    Ok(())
}

/// Mensajes de las tareas del coordinator hacia el bucle principal.
enum CoordMsg {
    /// Candidatos de un par (por rendezvous propio o por aviso del coordinator): solo pistas.
    Candidates(atenea_identity::DeviceId, Vec<SocketAddr>),
    /// Token de relay para un par (petición propia o aviso del coordinator).
    Relay(atenea_identity::DeviceId, crate::coord::RelayGrant),
    /// Relays de una concesión ya resueltos y verificados (IP literal).
    RelayVetted(atenea_identity::DeviceId, Vec<crate::relay::RelayEndpoint>),
}

#[derive(Clone, Default, serde::Serialize)]
struct CoordStatus {
    url: String,
    last_announce_ok_unix: Option<u64>,
    last_error: Option<&'static str>,
    announces: u64,
    events: u64,
    rendezvous: u64,
}

fn protected_endpoints(
    sync: Option<crate::config::SyncConfig>,
    coordinator: Option<crate::config::CoordinatorConfig>,
) -> Result<Vec<IpAddr>, String> {
    use std::net::ToSocketAddrs;
    let mut ips = Vec::new();
    for url in sync
        .as_ref()
        .map(|c| c.url.as_str())
        .into_iter()
        .chain(coordinator.as_ref().map(|c| c.url.as_str()))
    {
        let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
        let host = uri.host().ok_or("URL sin host")?;
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        ips.extend(
            (host, port)
                .to_socket_addrs()
                .map_err(|_| "no se pudo resolver un endpoint del plano de control")?
                .map(|a| a.ip()),
        );
    }
    if let Some(coord) = &coordinator {
        for stun in &coord.stun {
            ips.extend(
                stun.to_socket_addrs()
                    .map_err(|_| "no se pudo resolver STUN")?
                    .map(|a| a.ip()),
            );
        }
    }
    Ok(ips)
}

fn ipc_status(
    rt: &Runtime,
    sync: Option<crate::sync::Status>,
    coord: Option<CoordStatus>,
    relay: crate::relay::RelayStatus,
) -> serde_json::Value {
    let mut status =
        serde_json::to_value(rt.engine.status(Instant::now(), unix_now())).unwrap_or_default();
    status["sync"] = serde_json::to_value(sync).unwrap_or_default();
    status["coordinator"] = serde_json::to_value(coord).unwrap_or_default();
    status["relay"] = serde_json::to_value(relay).unwrap_or_default();
    let network = rt.netconf.status();
    status["routes"] = serde_json::to_value(&network.routes).unwrap_or_default();
    status["dns"] = serde_json::json!(network.dns);
    status["forwarding_ok"] = serde_json::json!(network.forwarding_ok);
    status
}

#[cfg(test)]
mod tests {
    use super::shutdown_runtime;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn shutdown_finishes_after_cleanup_without_waiting_for_blocking_http() {
        struct Cleanup(Arc<AtomicBool>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (started, wait_started) = mpsc::channel();
        runtime.spawn_blocking(move || {
            started.send(()).unwrap();
            std::thread::sleep(Duration::from_secs(20));
        });
        wait_started.recv_timeout(Duration::from_secs(3)).unwrap();
        let cleaned = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let result: Result<(), &str> = runtime.block_on(async {
            let _resources = Cleanup(cleaned.clone());
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert!(
            cleaned.load(Ordering::SeqCst),
            "los recursos se liberan antes de cerrar el runtime"
        );
        shutdown_runtime(runtime);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "el HTTP bloqueante no debe retrasar la parada"
        );
    }
}
