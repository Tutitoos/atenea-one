// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `atenea network`: varias redes por dispositivo, una instancia de agente y claves nuevas por red
//! (ADR-013 fase 2). Solo Unix; el CLI no se eleva: se ejecuta como root (`sudo atenea network …`).
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum NetworkCmd {
    /// Examine un código sin privilegios, disco ni red; nunca muestra el token.
    Inspect {
        code: String,
        #[arg(long)]
        json: bool,
    },
    /// Se une a una red con un código (`ATENEA1-…`) tras confirmarlo; claves nuevas por red.
    Join {
        code: String,
        /// Nombre de este dispositivo en la red. Sin él se usa el nombre del sistema.
        #[arg(long)]
        name: Option<String>,
        /// Grupo que puede leer el socket de estado de esta red; por defecto, solo root.
        #[arg(long)]
        status_gid: Option<u32>,
        /// No pregunta: confirma la unión (obligatorio sin terminal interactiva).
        #[arg(long)]
        yes: bool,
        /// Solo laboratorio: usa `http://` en lugar del `https://` del código.
        #[arg(long)]
        insecure_http: bool,
    },
    /// Lista las redes unidas y su estado (no requiere root).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Arranca la instancia de la red (id completo, prefijo único o nombre).
    Up { network: String },
    /// Para la instancia de la red.
    Down { network: String },
    /// Sale de la red: para la instancia, limpia rutas/DNS y BORRA sus claves (irreversible).
    Leave {
        network: String,
        #[arg(long)]
        yes: bool,
    },
}

#[cfg(not(unix))]
pub fn run(
    _root: Option<PathBuf>,
    _backend: crate::network_service::Backend,
    _cmd: NetworkCmd,
) -> Result<(), String> {
    Err("`atenea network` no está soportado todavía en Windows".into())
}

#[cfg(unix)]
pub fn run(
    root: Option<PathBuf>,
    backend: crate::network_service::Backend,
    cmd: NetworkCmd,
) -> Result<(), String> {
    use std::io::IsTerminal;
    let cmd = match cmd {
        NetworkCmd::Inspect { code, json } => {
            return imp::inspect(&code, json, &mut std::io::stdout().lock());
        }
        cmd => cmd,
    };
    let mut ctx = imp::Ctx::new(root)?;
    ctx.backend = backend;
    let interactive = std::io::stdin().is_terminal();
    let mut input = std::io::stdin().lock();
    match cmd {
        NetworkCmd::Join {
            code,
            name,
            status_gid,
            yes,
            insecure_http,
        } => imp::join(
            &ctx,
            &imp::JoinOpts {
                code,
                name,
                status_gid,
                yes,
                insecure_http,
            },
            &mut input,
            interactive,
        ),
        NetworkCmd::List { json } => imp::list(&ctx, json),
        NetworkCmd::Up { network } => imp::up(&ctx, &network),
        NetworkCmd::Down { network } => imp::down(&ctx, &network),
        NetworkCmd::Leave { network, yes } => {
            imp::leave(&ctx, &network, yes, &mut input, interactive)
        }
        NetworkCmd::Inspect { .. } => unreachable!(),
    }
}

#[cfg(unix)]
mod imp {
    use crate::network_service::{self, Backend, Service};
    use crate::{enroll, issue};
    use atenea_crypto::hex;
    use atenea_identity::anchor_fingerprint;
    use atenea_identity::join_code::JoinCode;
    use serde::{Deserialize, Serialize};
    use std::io::{BufRead, Write};
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// Metadatos NO secretos de una red unida (`network.json`). Nunca contienen el token.
    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Meta {
        pub network_id: String,
        pub name: String,
        pub server: String,
        pub url: String,
        pub root_anchor_fingerprint: String,
        pub device_id: String,
        pub device_fingerprint: String,
        pub interface: String,
        pub listen_port: u16,
        /// Segundos Unix.
        pub created: u64,
        /// `pendiente_de_aprobacion` o `aprobada` (lo que el CLI supo al unirse).
        pub join_status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub service_backend: Option<Service>,
    }

    pub struct Ctx {
        root: PathBuf,
        run: PathBuf,
        agent: Option<PathBuf>,
        pub backend: Backend,
        legacy: PathBuf,
    }

    impl Ctx {
        pub fn new(root: Option<PathBuf>) -> Result<Self, String> {
            let root = root
                .or_else(|| std::env::var_os("ATENEA_NETWORKS_DIR").map(PathBuf::from))
                .unwrap_or_else(|| {
                    PathBuf::from(if cfg!(target_os = "macos") {
                        "/Library/Application Support/Atenea/networks"
                    } else {
                        "/var/lib/atenea/networks"
                    })
                });
            let run = std::env::var_os("ATENEA_RUN_DIR").map_or_else(
                || {
                    PathBuf::from(if cfg!(target_os = "macos") {
                        "/var/run/atenea"
                    } else {
                        "/run/atenea"
                    })
                },
                PathBuf::from,
            );
            Ok(Self {
                root: std::path::absolute(root).map_err(|e| e.to_string())?,
                run: std::path::absolute(run).map_err(|e| e.to_string())?,
                agent: std::env::var_os("ATENEA_AGENT_BIN").map(PathBuf::from),
                backend: Backend::Auto,
                legacy: PathBuf::from(if cfg!(target_os = "macos") {
                    "/Library/Application Support/Atenea/agent.toml"
                } else {
                    "/etc/atenea/agent.toml"
                }),
            })
        }
        fn net_dir(&self, id: &str) -> PathBuf {
            self.root.join(id)
        }
        fn device_dir(&self, id: &str) -> PathBuf {
            self.net_dir(id).join("device")
        }
        fn run_dir(&self, id: &str) -> PathBuf {
            self.run.join(short(id))
        }
        fn agent_bin(&self) -> Result<PathBuf, String> {
            self.agent.clone().map_or_else(
                || {
                    Ok(std::env::current_exe()
                        .map_err(|e| e.to_string())?
                        .with_file_name("atenea-agent"))
                },
                Ok,
            )
        }
    }

    fn io_err(path: &Path, e: &std::io::Error) -> String {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!("{}: permiso denegado: ejecuta con sudo", path.display())
        } else {
            format!("{}: {e}", path.display())
        }
    }

    fn mkdir(path: &Path, mode: u32, recursive: bool) -> Result<(), String> {
        std::fs::DirBuilder::new()
            .recursive(recursive)
            .mode(mode)
            .create(path)
            .map_err(|e| io_err(path, &e))?;
        // El umask puede quitar bits; el modo pedido manda.
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
            .map_err(|e| io_err(path, &e))
    }
    /// Id corto de una red (últimos 8 hex del UUID): los UUIDv7 comparten el prefijo temporal.
    fn short(id: &str) -> &str {
        &id[28..]
    }

    fn uuid(b: &[u8; 16]) -> String {
        let h = hex::encode(b);
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }

    fn is_uuid_name(s: &str) -> bool {
        s.len() == 36
            && s.bytes().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => b == b'-',
                _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
            })
    }

    /// Redes unidas según `network.json`; un directorio ilegible se avisa y se omite.
    pub fn read_metas(ctx: &Ctx) -> Result<Vec<Meta>, String> {
        let entries = match std::fs::read_dir(&ctx.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err(&ctx.root, &e)),
        };
        let mut metas = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_uuid_name(&name) {
                continue;
            }
            let file = entry.path().join("network.json");
            match std::fs::read(&file)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Meta>(&b).map_err(|e| e.to_string()))
            {
                Ok(m) if m.network_id == name => metas.push(m),
                Ok(_) => eprintln!(
                    "aviso: {} no coincide con su directorio; se omite",
                    file.display()
                ),
                Err(e) => eprintln!("aviso: {}: {e}; se omite", file.display()),
            }
        }
        metas.sort_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then(a.network_id.cmp(&b.network_id))
        });
        Ok(metas)
    }

    fn legacy_config(ctx: &Ctx) -> Result<toml::Table, String> {
        match std::fs::read_to_string(&ctx.legacy) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("configuración heredada: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
            Err(e) => Err(io_err(&ctx.legacy, &e)),
        }
    }
    /// Interfaz y puerto libres; reserva también los recursos de legacy aunque esté detenida.
    pub fn allocate(metas: &[Meta], legacy: &toml::Table) -> Result<(String, u16), String> {
        let legacy_interface = legacy.get("interface").and_then(toml::Value::as_str);
        let legacy_port = legacy
            .get("listen")
            .and_then(toml::Value::as_str)
            .and_then(|s| s.parse::<std::net::SocketAddr>().ok())
            .map(|a| a.port());
        let interface = (0..1000)
            .map(|n| format!("atenea{n}"))
            .find(|name| {
                !metas.iter().any(|m| &m.interface == name)
                    && legacy_interface != Some(name.as_str())
                    && !Path::new("/sys/class/net").join(name).exists()
            })
            .ok_or("no queda ninguna interfaz atenea<N> libre")?;
        let port = (51820..=u16::MAX)
            .find(|p| {
                !metas.iter().any(|m| m.listen_port == *p)
                    && legacy_port != Some(*p)
                    && std::net::UdpSocket::bind(("0.0.0.0", *p)).is_ok()
            })
            .ok_or("no queda ningún puerto UDP libre desde 51820")?;
        Ok((interface, port))
    }

    /// Id completo, prefijo único (≥ 4 caracteres) o nombre único.
    pub fn resolve<'a>(metas: &'a [Meta], query: &str) -> Result<&'a Meta, String> {
        let q = query.trim().to_lowercase();
        if let Some(m) = metas.iter().find(|m| m.network_id == q) {
            return Ok(m);
        }
        let found: Vec<&Meta> = metas
            .iter()
            .filter(|m| {
                (q.len() >= 4 && (m.network_id.starts_with(&q) || m.network_id.ends_with(&q)))
                    || m.name.to_lowercase() == q
            })
            .collect();
        match found.as_slice() {
            [m] => Ok(m),
            [] => Err(format!(
                "ninguna red coincide con «{query}» (usa `atenea network list`)"
            )),
            many => Err(format!(
                "«{query}» es ambiguo; candidatas:\n{}",
                many.iter()
                    .map(|m| format!("  {} {}", m.network_id, m.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
        }
    }

    fn confirm(
        prompt: &str,
        yes: bool,
        interactive: bool,
        input: &mut dyn BufRead,
    ) -> Result<(), String> {
        if yes {
            return Ok(());
        }
        if !interactive {
            return Err("hace falta confirmación y no hay terminal interactiva: revisa los datos y repite con --yes".into());
        }
        print!("{prompt} [escribe «sí» para continuar]: ");
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        input.read_line(&mut line).map_err(|e| e.to_string())?;
        if matches!(
            line.trim().to_lowercase().as_str(),
            "sí" | "si" | "s" | "yes" | "y"
        ) {
            Ok(())
        } else {
            Err("cancelado: no se ha cambiado nada".into())
        }
    }

    pub struct JoinOpts {
        pub code: String,
        pub name: Option<String>,
        pub status_gid: Option<u32>,
        pub yes: bool,
        pub insecure_http: bool,
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// Crea claves, `agent.toml`, registra el dispositivo y escribe `network.json`.
    /// `accepted` en el error: el servidor ya aceptó la solicitud, no se debe deshacer en silencio.
    fn provision(
        ctx: &Ctx,
        code: &JoinCode,
        id: &str,
        url: &str,
        opts: &JoinOpts,
        (interface, port): (String, u16),
    ) -> Result<Meta, enroll::RegisterError> {
        let fail = |message: String| enroll::RegisterError {
            message,
            accepted: false,
        };
        let device = ctx.device_dir(id);
        mkdir(&device, 0o700, false).map_err(fail)?;
        let (_, _, fingerprint) = issue::device_create(&device).map_err(fail)?;
        let run = ctx.run_dir(id);
        enroll::pinned_config(
            url,
            &hex::encode(&code.network_id),
            &hex::encode(&code.root_anchor),
            &device,
            &device.join("agent.toml"),
            &interface,
            port,
            &run.join("control.sock"),
            &run.join("status.sock"),
            opts.status_gid,
            opts.insecure_http,
        )
        .map_err(fail)?;
        let (path, token) = match &code.token {
            Some(t) => ("/api/v1/enroll".to_string(), Some(t.as_str())),
            None => (format!("/api/v1/networks/{id}/join-requests"), None),
        };
        let registered = enroll::register(
            url,
            &path,
            token,
            &device,
            opts.name.as_deref(),
            opts.insecure_http,
        )?;
        let accepted = |message: String| enroll::RegisterError {
            message,
            accepted: true,
        };
        if registered.network_id.replace('-', "") != hex::encode(&code.network_id) {
            return Err(accepted(
                "el servidor respondió con otra red distinta a la del código".into(),
            ));
        }
        let meta = Meta {
            network_id: id.into(),
            name: code.name.clone(),
            server: server_of(&code.url),
            url: url.into(),
            root_anchor_fingerprint: anchor_fingerprint(&code.root_anchor),
            device_id: registered.device_id,
            device_fingerprint: fingerprint,
            interface,
            listen_port: port,
            created: now(),
            join_status: if registered.status == "approved" {
                "aprobada"
            } else {
                "pendiente_de_aprobacion"
            }
            .into(),
            service_backend: None,
        };
        write_meta(ctx, &meta).map_err(accepted)?;
        Ok(meta)
    }

    fn server_of(url: &str) -> String {
        url.strip_prefix("https://")
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or_default()
            .into()
    }

    #[derive(Serialize)]
    struct CodeInfo<'a> {
        name: &'a str,
        server: String,
        network_id: String,
        root_fingerprint: String,
        has_token: bool,
    }

    fn code_info(code: &JoinCode) -> CodeInfo<'_> {
        CodeInfo {
            name: &code.name,
            server: server_of(&code.url),
            network_id: uuid(&code.network_id),
            root_fingerprint: anchor_fingerprint(&code.root_anchor),
            has_token: code.token.is_some(),
        }
    }

    fn write_code_info(info: &CodeInfo<'_>, output: &mut dyn Write) -> Result<(), String> {
        writeln!(
            output,
            "Red:            {}\nServidor:       {}\nId de la red:   {}\nHuella de la raíz: {}",
            info.name, info.server, info.network_id, info.root_fingerprint
        )
        .map_err(|e| e.to_string())
    }

    pub fn inspect(text: &str, json: bool, output: &mut dyn Write) -> Result<(), String> {
        let code = JoinCode::decode(text).map_err(|e| format!("código no válido: {e}"))?;
        let info = code_info(&code);
        if json {
            writeln!(
                output,
                "{}",
                serde_json::to_string(&info).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())
        } else {
            write_code_info(&info, output)?;
            writeln!(output, "Lleva token:    {}", info.has_token).map_err(|e| e.to_string())
        }
    }

    fn write_meta(ctx: &Ctx, meta: &Meta) -> Result<(), String> {
        let path = ctx.net_dir(&meta.network_id).join("network.json");
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&path)
            .map_err(|e| io_err(&path, &e))?;
        f.write_all(&serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    /// `flock` del directorio, compartido por join/up/down/leave; no deja ficheros temporales ni
    /// depende de eliminar un candado para liberar. El kernel lo suelta también si el proceso cae.
    fn root_lock(ctx: &Ctx) -> Result<std::fs::File, String> {
        let file = std::fs::File::open(&ctx.root).map_err(|e| io_err(&ctx.root, &e))?;
        file.lock().map_err(|e| io_err(&ctx.root, &e))?;
        Ok(file)
    }

    fn save_backend(ctx: &Ctx, meta: &Meta, service: Service) -> Result<(), String> {
        let mut meta = meta.clone();
        meta.service_backend = Some(service);
        let path = ctx.net_dir(&meta.network_id).join("network.json");
        let tmp = path.with_file_name(format!(".network-{}.json", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&tmp)
            .map_err(|e| io_err(&tmp, &e))?;
        let result = (|| {
            f.write_all(&serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &path).map_err(|e| io_err(&path, &e))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    // Migra configuraciones de fase 2 al arrancar; solo para redes gestionadas (nunca legacy).
    fn ensure_resolver_owner(ctx: &Ctx, id: &str) -> Result<(), String> {
        let path = ctx.device_dir(id).join("agent.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| io_err(&path, &e))?;
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(owner) = table.get("resolver_owner") {
            return if owner.as_str() == Some(short(id)) {
                Ok(())
            } else {
                Err("resolver_owner no coincide con la red".into())
            };
        }
        table.insert("resolver_owner".into(), short(id).into());
        let tmp = path.with_file_name(format!(".agent-{}.toml", std::process::id()));
        issue::write_secret(&tmp, &toml::to_string(&table).map_err(|e| e.to_string())?)?;
        let result = std::fs::rename(&tmp, &path).map_err(|e| io_err(&path, &e));
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    pub fn join(
        ctx: &Ctx,
        opts: &JoinOpts,
        input: &mut dyn BufRead,
        interactive: bool,
    ) -> Result<(), String> {
        let code = JoinCode::decode(&opts.code).map_err(|e| format!("código no válido: {e}"))?;
        let info = code_info(&code);
        let url = if opts.insecure_http {
            code.url.replacen("https://", "http://", 1)
        } else {
            code.url.clone()
        };
        write_code_info(&info, &mut std::io::stdout().lock())?;
        let id = info.network_id;
        println!(
            "Código:         {}",
            if code.token.is_some() {
                "lleva token de un solo uso (equivale a una credencial)"
            } else {
                "de solicitud (sin token): un administrador deberá aprobar este equipo"
            }
        );
        println!(
            "AVISO: una red puede instalar rutas privadas y DNS para dominios concretos en este equipo; confirma que el código te lo dio alguien de confianza y compara la huella por otro canal."
        );
        confirm("¿Unirte a esta red?", opts.yes, interactive, input)?;
        if !ctx.root.exists() {
            mkdir(&ctx.root, 0o755, true)?;
        }
        let _lock = root_lock(ctx)?;
        let metas = read_metas(ctx)?;
        if let Some(m) = metas.iter().find(|m| m.network_id == id) {
            return Err(format!(
                "ya estás unido a la red «{}» ({id}); no se ha cambiado nada",
                m.name
            ));
        }
        if let Some(m) = metas.iter().find(|m| short(&m.network_id) == short(&id)) {
            return Err(format!(
                "el id corto {} ya lo usa la red «{}»; no se puede unir sin colisión de sockets",
                short(&id),
                m.name
            ));
        }
        let net = ctx.net_dir(&id);
        // 0711: `network.json` (público) se lee sin root; las claves quedan en `device/` (0700).
        mkdir(&net, 0o711, false)?;
        let alloc = match legacy_config(ctx).and_then(|legacy| allocate(&metas, &legacy)) {
            Ok(a) => a,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&net);
                return Err(e);
            }
        };
        match provision(ctx, &code, &id, &url, opts, alloc) {
            Ok(meta) => {
                println!(
                    "red={id}\ndispositivo={}\nhuella_dispositivo={}",
                    meta.device_id, meta.device_fingerprint
                );
                println!("estado={}", meta.join_status);
                println!(
                    "Estado: {}. Compara la huella del dispositivo con la que ve el administrador.",
                    if meta.join_status == "aprobada" {
                        "aprobada"
                    } else {
                        "pendiente de aprobación"
                    }
                );
                println!("Siguiente paso: sudo atenea network up {}", short(&id));
                Ok(())
            }
            Err(e) if !e.accepted => {
                std::fs::remove_dir_all(&net).map_err(|x| io_err(&net, &x))?;
                Err(format!("{} (no se ha dejado nada en el disco)", e.message))
            }
            Err(e) => Err(format!(
                "{}. El servidor YA aceptó la solicitud: se conserva {} (claves nuevas del dispositivo); \
                 si no puedes completarla, usa `atenea network leave {id}` y pide al administrador que revoque el dispositivo",
                e.message,
                net.display()
            )),
        }
    }

    enum State {
        Stopped,
        NoConfig,
        Connected,
        Conflict(String),
        Unknown(String),
    }

    fn probe(ctx: &Ctx, m: &Meta) -> State {
        let sock = ctx.run_dir(&m.network_id).join("status.sock");
        if !sock.exists() {
            return State::Stopped;
        }
        match crate::ask(&sock, "status") {
            Ok(text) => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                let cfg = &v["config"];
                if matches!(
                    cfg["last_error"].as_str(),
                    Some("network_overlap" | "dns_domain_conflict")
                ) {
                    State::Conflict(format!(
                        "{}: {}",
                        cfg["last_error"].as_str().unwrap_or("conflicto"),
                        cfg["last_error_detail"]
                            .as_str()
                            .unwrap_or("solapa con otra red o ruta local")
                    ))
                } else if cfg["version"].as_u64().unwrap_or(0) > 0 {
                    State::Connected
                } else {
                    State::NoConfig
                }
            }
            Err(e) if e.contains("Permission denied") => {
                State::Unknown("sin permiso para leer el estado: ejecuta con sudo".into())
            }
            Err(_) => State::Stopped,
        }
    }

    impl State {
        fn code(&self) -> &'static str {
            match self {
                Self::Stopped => "detenida",
                Self::NoConfig => "iniciada_sin_configuracion",
                Self::Connected => "conectada",
                Self::Conflict(_) => "conflicto",
                Self::Unknown(_) => "desconocida",
            }
        }
        fn text(&self) -> String {
            match self {
                Self::Stopped => "detenida".into(),
                Self::NoConfig => "iniciada, sin configuración".into(),
                Self::Connected => "conectada".into(),
                Self::Conflict(d) => format!("conflicto: {d}"),
                Self::Unknown(d) => format!("desconocida ({d})"),
            }
        }
    }

    fn backend(ctx: &Ctx, m: &Meta) -> Service {
        let saved = m.service_backend.or_else(|| {
            if ctx.run_dir(&m.network_id).join("status.sock").exists() {
                Some(Service::Process)
            } else {
                None
            }
        });
        network_service::choose(ctx.backend, saved)
    }

    pub fn list_json(ctx: &Ctx) -> Result<serde_json::Value, String> {
        Ok(read_metas(ctx)?
            .iter()
            .map(|m| {
                let state = probe(ctx, m);
                let mut v = serde_json::to_value(m).expect("serializable");
                v["state"] = state.code().into();
                v["backend"] = backend(ctx, m).name().into();
                if let State::Conflict(d) | State::Unknown(d) = &state {
                    v["detail"] = d.clone().into();
                }
                v
            })
            .collect())
    }

    pub fn list(ctx: &Ctx, json: bool) -> Result<(), String> {
        let metas = read_metas(ctx)?;
        if json {
            println!("{}", list_json(ctx)?);
        } else if metas.is_empty() {
            println!("No hay redes unidas.");
        }
        if !json {
            for m in &metas {
                println!(
                    "{}  {}  {}  {}  {}  {}  {}",
                    m.name,
                    short(&m.network_id),
                    m.server,
                    m.interface,
                    m.listen_port,
                    probe(ctx, m).text(),
                    backend(ctx, m).name()
                );
            }
        }
        Ok(())
    }

    pub fn up(ctx: &Ctx, query: &str) -> Result<(), String> {
        let _lock = root_lock(ctx)?;
        let metas = read_metas(ctx)?;
        let m = resolve(&metas, query)?;
        let service = backend(ctx, m);
        if m.service_backend.is_none()
            && service != Service::Process
            && !matches!(probe(ctx, m), State::Stopped)
        {
            return Err(
                "instancia de fase 2 en modo proceso: párala con --backend process antes de migrar"
                    .into(),
            );
        }
        if m.service_backend.is_some_and(|old| old != service)
            && !matches!(probe(ctx, m), State::Stopped)
        {
            return Err(
                "la red está iniciada con otro gestor: usa `down --backend auto` antes de cambiar"
                    .into(),
            );
        }
        // Una instancia proceso de fase 2 conserva el owner legacy hasta pararse: no cambiar su
        // configuración mientras vive, porque cleanup debe usar la misma marca DNS que al arrancar.
        if service == Service::Process && !matches!(probe(ctx, m), State::Stopped) {
            println!("«{}» ya estaba iniciada", m.name);
            return Ok(());
        }
        ensure_resolver_owner(ctx, &m.network_id)?;
        if service != Service::Process {
            network_service::start(service, &ctx.root, &m.network_id)?;
            save_backend(ctx, m, service)?;
            println!("red «{}» iniciada mediante {}", m.name, service.name());
            return Ok(());
        }
        let run = ctx.run_dir(&m.network_id);
        for dir in [&ctx.run, &run] {
            if !dir.is_dir() {
                mkdir(dir, 0o755, false)?;
            }
        }
        let device = ctx.device_dir(&m.network_id);
        crate::up(
            &device.join("agent.toml"),
            Some(ctx.agent_bin()?),
            &device.join("agent.log"),
        )?;
        save_backend(ctx, m, Service::Process)?;
        println!("red «{}» iniciada ({})", m.name, m.interface);
        Ok(())
    }

    /// Para la instancia; `Ok(false)` si no estaba iniciada. Espera a que el socket desaparezca.
    fn stop(ctx: &Ctx, m: &Meta) -> Result<bool, String> {
        let service = backend(ctx, m);
        if m.service_backend.is_some_and(|old| old != service) {
            return Err("la red usa otro gestor; utiliza --backend auto para pararla".into());
        }
        if service != Service::Process {
            network_service::stop(service, &m.network_id)?;
            return Ok(true);
        }
        let run = ctx.run_dir(&m.network_id);
        let control = run.join("control.sock");
        if !control.exists()
            || crate::ask(&run.join("status.sock"), "status").is_err()
                && crate::ask(&control, "status").is_err()
        {
            return Ok(false);
        }
        crate::ask(&control, "stop").map_err(|e| {
            if e.contains("Permission denied") {
                format!("{e}: ejecuta con sudo")
            } else {
                e
            }
        })?;
        for _ in 0..50 {
            if crate::ask(&control, "status").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(true)
    }

    pub fn down(ctx: &Ctx, query: &str) -> Result<(), String> {
        let _lock = root_lock(ctx)?;
        let metas = read_metas(ctx)?;
        let m = resolve(&metas, query)?;
        println!(
            "red «{}» {}",
            m.name,
            if stop(ctx, m)? {
                "detenida"
            } else {
                "ya estaba detenida"
            }
        );
        Ok(())
    }

    pub fn leave(
        ctx: &Ctx,
        query: &str,
        yes: bool,
        input: &mut dyn BufRead,
        interactive: bool,
    ) -> Result<(), String> {
        let metas = read_metas(ctx)?;
        let m = resolve(&metas, query)?;
        let net = ctx.net_dir(&m.network_id);
        println!(
            "Vas a salir de la red «{}» ({}). Se detendrá su instancia, se limpiarán sus rutas y DNS y se BORRARÁN las claves de este dispositivo en esa red: es irreversible.",
            m.name, m.network_id
        );
        confirm("¿Salir de la red?", yes, interactive, input)?;
        let _lock = root_lock(ctx)?;
        let metas = read_metas(ctx)?;
        let m = resolve(&metas, query)?;
        let meta = std::fs::symlink_metadata(&net).map_err(|e| io_err(&net, &e))?;
        if !meta.is_dir() {
            return Err(format!(
                "{}: no es un directorio; no se borra",
                net.display()
            ));
        }
        stop(ctx, m)?;
        let agent = ctx.agent_bin()?;
        let status = Command::new(&agent)
            .args(["cleanup", "--config"])
            .arg(ctx.device_dir(&m.network_id).join("agent.toml"))
            .status()
            .map_err(|e| format!("{}: {e}", agent.display()))?;
        if !status.success() {
            return Err(format!(
                "la limpieza del agente falló ({status}); no se ha borrado nada de la red"
            ));
        }
        network_service::remove(backend(ctx, m), &m.network_id)?;
        std::fs::remove_dir_all(&net).map_err(|e| io_err(&net, &e))?;
        let _ = std::fs::remove_dir_all(ctx.run_dir(&m.network_id));
        println!(
            "Has salido de la red «{}»; sus claves se han borrado.",
            m.name
        );
        println!(
            "El dispositivo SIGUE registrado en el servidor: un administrador debe revocarlo. Huella del dispositivo: {} (id {}).",
            m.device_fingerprint, m.device_id
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Cursor, Read};
        use std::net::TcpListener;

        const SERVER_ANCHOR: [u8; 32] = [9; 32];

        fn temp(tag: &str) -> PathBuf {
            let n = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = Path::new("/tmp").join(format!("atenea-net-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            p
        }

        fn ctx(dir: &Path) -> Ctx {
            Ctx {
                root: dir.join("redes"),
                run: dir.join("run"),
                agent: Some(
                    ["/usr/bin/true", "/bin/true"]
                        .iter()
                        .map(PathBuf::from)
                        .find(|p| p.exists())
                        .unwrap(),
                ),
                backend: Backend::Process,
                legacy: dir.join("legacy.toml"),
            }
        }

        fn code_text(port: u16, token: Option<&str>, net: u8) -> String {
            JoinCode {
                url: format!("https://127.0.0.1:{port}"),
                network_id: [net; 16],
                root_anchor: [7; 32],
                name: "Red de ejemplo".into(),
                token: token.map(str::to_string),
            }
            .encode()
            .unwrap()
        }

        /// Servidor HTTP de una petición; devuelve el puerto y la petición recibida.
        fn serve(status: u16, body: String) -> (u16, std::thread::JoinHandle<String>) {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let h = std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text[..i]
                            .to_lowercase()
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= i + 4 + len || n == 0 {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                String::from_utf8_lossy(&buf).into_owned()
            });
            (port, h)
        }

        fn opts(code: String, yes: bool) -> JoinOpts {
            JoinOpts {
                code,
                name: Some("equipo-prueba".into()),
                yes,
                status_gid: None,
                insecure_http: true,
            }
        }

        fn created(net: u8) -> String {
            format!(
                r#"{{"device_id":"dd","network_id":"{}","status":"pending","root_anchor":"{}"}}"#,
                hex::encode(&[net; 16]),
                hex::encode(&SERVER_ANCHOR)
            )
        }

        fn files(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().is_dir() {
                    files(&e.path(), out);
                } else {
                    out.push(e.path());
                }
            }
        }

        fn fake_meta(id: &str, name: &str, iface: &str, port: u16, created: u64) -> Meta {
            Meta {
                network_id: id.into(),
                name: name.into(),
                server: "control.example.test".into(),
                url: "https://control.example.test".into(),
                root_anchor_fingerprint: "aaaa".into(),
                device_id: "dev".into(),
                device_fingerprint: "ffff".into(),
                interface: iface.into(),
                listen_port: port,
                created,
                join_status: "aprobada".into(),
                service_backend: None,
            }
        }

        fn put_meta(c: &Ctx, m: &Meta) {
            let d = c.device_dir(&m.network_id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("noise.key"), "clave").unwrap();
            write_meta(c, m).unwrap();
        }

        const A: &str = "11111111-1111-1111-1111-111111111111";
        const B: &str = "11112222-2222-2222-2222-222222222222";
        const C: &str = "33333333-3333-3333-3333-333333333333";

        #[test]
        fn inspect_reports_only_public_fields_without_touching_disk() {
            let d = temp("inspect");
            let root = d.join("redes");
            for token in [None, Some("TOKEN_NO_MOSTRAR")] {
                for json in [false, true] {
                    let mut output = Vec::new();
                    inspect(&code_text(443, token, 1), json, &mut output).unwrap();
                    let text = String::from_utf8(output).unwrap();
                    assert!(!text.contains("TOKEN_NO_MOSTRAR"));
                    if json {
                        assert_eq!(text.lines().count(), 1);
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
                            serde_json::json!({
                                "name": "Red de ejemplo",
                                "server": "127.0.0.1:443",
                                "network_id": "01010101-0101-0101-0101-010101010101",
                                "root_fingerprint": anchor_fingerprint(&[7; 32]),
                                "has_token": token.is_some(),
                            })
                        );
                    } else {
                        assert!(text.contains("Red de ejemplo"));
                        assert!(text.contains("127.0.0.1:443"));
                        assert!(text.contains("01010101-0101-0101-0101-010101010101"));
                        assert!(text.contains(&anchor_fingerprint(&[7; 32])));
                    }
                    crate::network::run(
                        Some(root.clone()),
                        Backend::Auto,
                        crate::network::NetworkCmd::Inspect {
                            code: code_text(443, token, 1),
                            json,
                        },
                    )
                    .unwrap();
                }
            }
            assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn inspect_rejects_tampered_and_truncated_codes_without_output() {
            let valid = code_text(443, Some("TOKEN_NO_MOSTRAR"), 1);
            let mut tampered = valid.clone();
            tampered.replace_range(12..13, if &valid[12..13] == "A" { "B" } else { "A" });
            for code in [tampered, valid[..valid.len() - 8].into()] {
                let mut output = Vec::new();
                let error = inspect(&code, true, &mut output).unwrap_err();
                assert!(error.starts_with("código no válido: "), "{error}");
                assert!(output.is_empty());
                assert!(!error.contains("TOKEN_NO_MOSTRAR"));
            }
        }

        #[test]
        fn network_cli_accepts_inspect_and_optional_u32_status_gid() {
            use clap::Parser;
            #[derive(Parser)]
            struct Args {
                #[command(subcommand)]
                command: crate::network::NetworkCmd,
            }
            let inspect =
                Args::try_parse_from(["network", "inspect", "ATENEA1-…", "--json"]).unwrap();
            assert!(matches!(
                inspect.command,
                crate::network::NetworkCmd::Inspect { json: true, .. }
            ));
            for (flags, expected) in [
                (vec![], None),
                (vec!["--status-gid", "545"], Some(545)),
                (vec!["--status-gid", "4294967295"], Some(u32::MAX)),
            ] {
                let parsed =
                    Args::try_parse_from(["network", "join", "ATENEA1-…"].into_iter().chain(flags))
                        .unwrap();
                let crate::network::NetworkCmd::Join { status_gid, .. } = parsed.command else {
                    panic!("se esperaba join");
                };
                assert_eq!(status_gid, expected);
            }
            assert!(
                Args::try_parse_from([
                    "network",
                    "join",
                    "ATENEA1-…",
                    "--status-gid",
                    "4294967296"
                ])
                .is_err()
            );
        }

        #[test]
        fn join_writes_optional_status_gid_using_agent_contract() {
            for gid in [None, Some(545), Some(u32::MAX)] {
                let d = temp("status-gid");
                let c = ctx(&d);
                let id = uuid(&[1; 16]);
                let (port, server) = serve(201, created(1));
                let mut options = opts(code_text(port, None, 1), true);
                options.status_gid = gid;
                join(&c, &options, &mut Cursor::new(""), false).unwrap();
                server.join().unwrap();
                let text = std::fs::read_to_string(c.device_dir(&id).join("agent.toml")).unwrap();
                let config = enroll::agent_config_contract::Config::parse(&text).unwrap();
                assert_eq!(config.status_gid, gid);
                assert_eq!(
                    config.status_socket,
                    Some(c.run_dir(&id).join("status.sock"))
                );
                let table: toml::Table = toml::from_str(&text).unwrap();
                assert_eq!(table.contains_key("status_gid"), gid.is_some());
                std::fs::remove_dir_all(d).unwrap();
            }
        }

        #[test]
        fn concurrent_joins_serialize_allocation_and_keep_distinct_keys() {
            let d = temp("concurrente");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let mut workers = Vec::new();
            let mut servers = Vec::new();
            for n in [1, 2] {
                let (port, server) = serve(201, created(n));
                servers.push(server);
                let b = barrier.clone();
                let c = ctx(&d);
                workers.push(std::thread::spawn(move || {
                    b.wait();
                    join(
                        &c,
                        &opts(code_text(port, None, n), true),
                        &mut Cursor::new(""),
                        false,
                    )
                }));
            }
            for w in workers {
                w.join().unwrap().unwrap();
            }
            for s in servers {
                s.join().unwrap();
            }
            let c = ctx(&d);
            let metas = read_metas(&c).unwrap();
            assert_eq!(metas.len(), 2);
            assert_ne!(metas[0].interface, metas[1].interface);
            assert_ne!(metas[0].listen_port, metas[1].listen_port);
            assert_ne!(
                std::fs::read(c.device_dir(&metas[0].network_id).join("noise.key")).unwrap(),
                std::fs::read(c.device_dir(&metas[1].network_id).join("noise.key")).unwrap()
            );
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn legacy_resources_are_reserved_even_when_it_is_stopped() {
            let legacy: toml::Table =
                toml::from_str("interface = 'atenea0'\nlisten = '0.0.0.0:51820'\n").unwrap();
            let (interface, port) = allocate(&[], &legacy).unwrap();
            assert_eq!(interface, "atenea1");
            assert!(port > 51820);
        }

        #[test]
        fn invalid_code_does_not_touch_disk() {
            let d = temp("invalido");
            let c = ctx(&d);
            let mut bad = code_text(1, None, 1);
            bad.replace_range(12..13, if &bad[12..13] == "A" { "B" } else { "A" });
            let e = join(&c, &opts(bad, true), &mut Cursor::new(""), false).unwrap_err();
            assert!(e.contains("código no válido"), "{e}");
            assert!(!c.root.exists());
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn refuses_without_yes_and_terminal_and_when_declined() {
            let d = temp("confirma");
            let c = ctx(&d);
            let code = code_text(1, None, 1);
            let e = join(
                &c,
                &opts(code.clone(), false),
                &mut Cursor::new("sí\n"),
                false,
            )
            .unwrap_err();
            assert!(e.contains("--yes"), "{e}");
            let e = join(&c, &opts(code, false), &mut Cursor::new("no\n"), true).unwrap_err();
            assert!(e.contains("cancelado"), "{e}");
            assert!(!c.root.exists());
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn token_join_pins_code_anchor_never_stores_token_and_rejects_duplicate() {
            let d = temp("token");
            let c = ctx(&d);
            let (port, h) = serve(201, created(1));
            let code = code_text(port, Some("TOKEN_SECRETO_1"), 1);
            join(&c, &opts(code.clone(), true), &mut Cursor::new(""), false).unwrap();
            let request = h.join().unwrap();
            assert!(request.contains("POST /api/v1/enroll "), "{request}");
            assert!(request.contains("TOKEN_SECRETO_1"));
            let id = uuid(&[1; 16]);
            let mut all = Vec::new();
            files(&c.root, &mut all);
            assert!(all.len() >= 5, "{all:?}");
            for f in &all {
                let bytes = std::fs::read(f).unwrap();
                assert!(
                    !String::from_utf8_lossy(&bytes).contains("TOKEN_SECRETO_1"),
                    "token en {}",
                    f.display()
                );
            }
            let toml = std::fs::read_to_string(c.device_dir(&id).join("agent.toml")).unwrap();
            assert!(toml.contains(&format!("trust_anchor = \"{}\"", hex::encode(&[7u8; 32]))));
            assert!(!toml.contains(&hex::encode(&SERVER_ANCHOR)));
            assert!(toml.contains(&format!("{}/control.sock", c.run_dir(&id).display())));
            let metas = read_metas(&c).unwrap();
            assert_eq!(metas.len(), 1);
            assert_eq!(
                (metas[0].interface.as_str(), metas[0].join_status.as_str()),
                ("atenea0", "pendiente_de_aprobacion")
            );
            let e = join(&c, &opts(code, true), &mut Cursor::new(""), false).unwrap_err();
            assert!(e.contains("ya estás unido"), "{e}");
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn request_join_uses_join_requests_endpoint_and_keys_differ_per_network() {
            let d = temp("solicitud");
            let c = ctx(&d);
            let mut keys = Vec::new();
            for net in [1u8, 2] {
                let (port, h) = serve(201, created(net));
                join(
                    &c,
                    &opts(code_text(port, None, net), true),
                    &mut Cursor::new(""),
                    false,
                )
                .unwrap();
                let request = h.join().unwrap();
                let id = uuid(&[net; 16]);
                assert!(
                    request.contains(&format!("POST /api/v1/networks/{id}/join-requests ")),
                    "{request}"
                );
                assert!(!request.contains("\"token\""));
                keys.push(std::fs::read(c.device_dir(&id).join("noise.key")).unwrap());
            }
            assert_ne!(keys[0], keys[1]);
            let metas = read_metas(&c).unwrap();
            assert_ne!(metas[0].interface, metas[1].interface);
            assert_ne!(metas[0].listen_port, metas[1].listen_port);
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn server_failures_roll_back_everything() {
            let d = temp("rollback");
            let c = ctx(&d);
            for (status, body, expected) in [
                (404, "{}".to_string(), "no admite solicitudes"),
                (
                    429,
                    r#"{"error":{"code":"too_many_pending"}}"#.to_string(),
                    "demasiadas solicitudes pendientes",
                ),
            ] {
                let (port, h) = serve(status, body);
                let e = join(
                    &c,
                    &opts(code_text(port, None, 2), true),
                    &mut Cursor::new(""),
                    false,
                )
                .unwrap_err();
                h.join().unwrap();
                assert!(e.contains(expected), "{e}");
                assert!(!c.net_dir(&uuid(&[2; 16])).exists());
            }
            let closed = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let e = join(
                &c,
                &opts(code_text(closed, None, 2), true),
                &mut Cursor::new(""),
                false,
            )
            .unwrap_err();
            assert!(e.contains("no se pudo contactar"), "{e}");
            assert!(read_metas(&c).unwrap().is_empty());
            let mut all = Vec::new();
            files(&c.root, &mut all);
            assert!(all.is_empty(), "{all:?}");
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn allocation_avoids_metadata_and_busy_ports() {
            let a = fake_meta(A, "a", "atenea0", 51820, 1);
            let (iface, port) = allocate(std::slice::from_ref(&a), &toml::Table::new()).unwrap();
            assert_eq!(iface, "atenea1");
            assert!(port > 51820);
            let _busy = std::net::UdpSocket::bind(("0.0.0.0", port)).unwrap();
            let (_, next) = allocate(&[a], &toml::Table::new()).unwrap();
            assert_ne!(next, port);
        }

        #[test]
        fn resolve_by_prefix_name_and_ambiguity() {
            let metas = [
                fake_meta(A, "Casa", "atenea0", 51820, 1),
                fake_meta(B, "Oficina", "atenea1", 51821, 2),
                fake_meta(C, "Casa", "atenea2", 51822, 3),
            ];
            assert!(resolve(&metas, "1111-").is_err());
            assert_eq!(resolve(&metas, "11112222").unwrap().name, "Oficina");
            assert_eq!(resolve(&metas, "oficina").unwrap().network_id, B);
            let e = resolve(&metas, "1111").unwrap_err();
            assert!(
                e.contains("ambiguo") && e.contains(A) && e.contains(B),
                "{e}"
            );
            let e = resolve(&metas, "casa").unwrap_err();
            assert!(e.contains(A) && e.contains(C), "{e}");
            assert!(resolve(&metas, "nada").is_err());
        }

        #[test]
        fn short_id_collision_is_refused_before_touching_disk() {
            let d = temp("colision");
            let c = ctx(&d);
            put_meta(
                &c,
                &fake_meta(
                    "aaaaaaaa-aaaa-aaaa-aaaa-aaaa12345678",
                    "x",
                    "atenea0",
                    51820,
                    1,
                ),
            );
            let mut network_id = [0xbb; 16];
            network_id[12..].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
            let code = JoinCode {
                url: "https://127.0.0.1:1".into(),
                network_id,
                root_anchor: [7; 32],
                name: "otra".into(),
                token: None,
            };
            let e = join(
                &c,
                &opts(code.encode().unwrap(), true),
                &mut Cursor::new(""),
                false,
            )
            .unwrap_err();
            assert!(e.contains("id corto 12345678"), "{e}");
            assert_eq!(std::fs::read_dir(&c.root).unwrap().count(), 1);
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn list_json_reports_stopped_networks() {
            let d = temp("lista");
            let c = ctx(&d);
            put_meta(&c, &fake_meta(A, "Casa", "atenea0", 51820, 1));
            let v = list_json(&c).unwrap();
            assert_eq!(v[0]["state"], "detenida");
            assert_eq!(v[0]["name"], "Casa");
            assert_eq!(v[0]["listen_port"], 51820);
            assert_eq!(v[0]["interface"], "atenea0");
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn up_keeps_running_phase_two_configuration_and_owner_unchanged() {
            use std::os::unix::net::UnixListener;
            let d = temp("fase-dos-activa");
            let c = ctx(&d);
            put_meta(&c, &fake_meta(A, "Casa", "atenea0", 51820, 1));
            let path = c.device_dir(A).join("agent.toml");
            std::fs::write(&path, "interface = 'atenea0'\n").unwrap();
            std::fs::create_dir_all(c.run_dir(A)).unwrap();
            let listener = UnixListener::bind(c.run_dir(A).join("status.sock")).unwrap();
            let h = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&s).read_line(&mut line).unwrap();
                writeln!(s, r#"{{"config":{{"version":1}}}}"#).unwrap();
            });
            up(&c, A).unwrap();
            h.join().unwrap();
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "interface = 'atenea0'\n"
            );
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn status_socket_drives_state() {
            use std::os::unix::net::UnixListener;
            let d = temp("estado");
            let c = ctx(&d);
            let m = fake_meta(A, "Casa", "atenea0", 51820, 1);
            put_meta(&c, &m);
            std::fs::create_dir_all(c.run_dir(A)).unwrap();
            for (reply, expected) in [
                (
                    r#"{"config":{"version":0,"last_error":null}}"#,
                    "iniciada_sin_configuracion",
                ),
                (r#"{"config":{"version":3,"last_error":null}}"#, "conectada"),
                (
                    r#"{"config":{"version":0,"last_error":"network_overlap","last_error_detail":"10.77.0.0/24"}}"#,
                    "conflicto",
                ),
            ] {
                let sock = c.run_dir(A).join("status.sock");
                let _ = std::fs::remove_file(&sock);
                let l = UnixListener::bind(&sock).unwrap();
                let h = std::thread::spawn(move || {
                    let (mut s, _) = l.accept().unwrap();
                    let mut line = String::new();
                    std::io::BufReader::new(&s).read_line(&mut line).unwrap();
                    writeln!(s, "{reply}").unwrap();
                });
                assert_eq!(list_json(&c).unwrap()[0]["state"], expected);
                h.join().unwrap();
            }
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn leave_deletes_only_its_own_directory() {
            let d = temp("salir");
            let c = ctx(&d);
            put_meta(&c, &fake_meta(A, "Casa", "atenea0", 51820, 1));
            put_meta(&c, &fake_meta(C, "Oficina", "atenea1", 51821, 2));
            std::fs::create_dir_all(c.run_dir(C)).unwrap();
            let e = leave(&c, "casa", false, &mut Cursor::new(""), false).unwrap_err();
            assert!(e.contains("--yes"));
            assert!(c.net_dir(A).exists());
            leave(&c, "casa", true, &mut Cursor::new(""), false).unwrap();
            assert!(!c.net_dir(A).exists());
            assert!(c.device_dir(C).join("noise.key").exists());
            assert!(c.net_dir(C).join("network.json").exists());
            assert!(c.run_dir(C).exists());
            std::fs::remove_dir_all(d).unwrap();
        }
    }
}
