// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `atenea network` en Windows (ADR-017 §2): `list` y `legacy` leen sin privilegios el índice público
//! `%ProgramData%\Atenea\public\<UUID>.json`, que el árbol privado de las redes no expone.
use crate::network_common::{Meta, is_uuid_name};
use std::path::Path;

/// Redes del índice público; un fichero ilegible o que no coincide con su nombre se avisa y se omite.
pub fn read_metas(public: &Path) -> Result<Vec<Meta>, String> {
    let entries = match std::fs::read_dir(public) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", public.display())),
    };
    let mut metas = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|id| is_uuid_name(id))
        else {
            continue;
        };
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Meta>(&b).map_err(|e| e.to_string()))
        {
            Ok(m) if m.network_id == id => metas.push(m),
            Ok(_) => eprintln!(
                "aviso: {} no coincide con su nombre; se omite",
                path.display()
            ),
            Err(e) => eprintln!("aviso: {}: {e}; se omite", path.display()),
        }
    }
    metas.sort_by(|a, b| {
        a.created
            .cmp(&b.created)
            .then(a.network_id.cmp(&b.network_id))
    });
    Ok(metas)
}

/// Primer puerto UDP libre desde 51820 que no use otra red, el cliente heredado ni otro proceso.
pub fn allocate_port(used: &[u16], legacy: Option<u16>) -> Result<u16, String> {
    (51820..=u16::MAX)
        .find(|p| {
            !used.contains(p)
                && legacy != Some(*p)
                && std::net::UdpSocket::bind(("0.0.0.0", *p)).is_ok()
        })
        .ok_or_else(|| "no queda ningún puerto UDP libre desde 51820".into())
}

/// Puerto de `listen` en el `agent.toml` del cliente heredado.
pub fn legacy_port(text: &str) -> Option<u16> {
    toml::from_str::<toml::Table>(text)
        .ok()?
        .get("listen")?
        .as_str()?
        .parse::<std::net::SocketAddr>()
        .ok()
        .map(|a| a.port())
}

#[cfg(windows)]
pub use system::*;

#[cfg(windows)]
mod system {
    use super::{allocate_port, legacy_port, read_metas};
    use crate::network_common::{
        Meta, State, code_info, confirm, now, offline_warning, resolve, server_of, short,
        state_from_status, write_code_info,
    };
    use crate::network_service::Service;
    use crate::{enroll, issue};
    use atenea_crypto::hex;
    use atenea_identity::anchor_fingerprint;
    use atenea_identity::join_code::JoinCode;
    use atenea_windows::{
        NetworkService, ServiceStatus, check_trusted_binary, create_public_dir,
        delete_network_service, is_elevated, replace_public, service_status, start_network_service,
        stop_network_service,
    };
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::process::Command;

    /// Servicio heredado de ADR-008, anterior a las redes por instancia.
    const LEGACY_SERVICE: &str = "AteneaAgent";

    pub struct Ctx {
        /// `%ProgramData%\Atenea`: configuración y claves del cliente, solo SYSTEM y Administradores.
        data: PathBuf,
        /// `%ProgramData%\Atenea\public`: metadatos no secretos de cada red, legibles por los usuarios.
        public: PathBuf,
        /// `%ProgramData%\Atenea\networks`: claves y configuración por red, solo SYSTEM y Administradores.
        networks: PathBuf,
    }

    impl Ctx {
        pub fn new() -> Result<Self, String> {
            // Ninguna de estas rutas admite sustitución por variable de entorno: `join` y `leave` escriben y
            // borran en ellas como administrador, y de `networks` sale la configuración que ejecuta un
            // servicio LocalSystem.
            let data = PathBuf::from(
                std::env::var_os("ProgramData").ok_or("falta la variable ProgramData")?,
            )
            .join("Atenea");
            Ok(Self {
                public: data.join("public"),
                networks: data.join("networks"),
                data,
            })
        }

        fn net_dir(&self, id: &str) -> PathBuf {
            self.networks.join(id)
        }

        fn device_dir(&self, id: &str) -> PathBuf {
            self.net_dir(id).join("device")
        }

        /// Puerto UDP del cliente heredado (`AteneaAgent`), aunque esté parado: no se reutiliza.
        fn legacy_port(&self) -> Option<u16> {
            std::fs::read_to_string(self.data.join("agent.toml"))
                .ok()
                .and_then(|text| legacy_port(&text))
        }

        /// El servicio de una red, con el binario fijo `%ProgramFiles%\Atenea\atenea-agent.exe` (ADR-017 §7).
        fn service_spec_unchecked(&self, m: &Meta) -> Result<NetworkService, String> {
            let program_files =
                std::env::var_os("ProgramFiles").ok_or("falta la variable ProgramFiles")?;
            Ok(NetworkService {
                name: service_name(&m.network_id),
                display_name: format!("Atenea One ({})", short(&m.network_id)),
                executable: PathBuf::from(program_files)
                    .join("Atenea")
                    .join("atenea-agent.exe"),
                config: self.device_dir(&m.network_id).join("agent.toml"),
            })
        }

        /// Igual, exigiendo que la red esté completa (tiene `agent.toml`).
        fn service_spec(&self, m: &Meta) -> Result<NetworkService, String> {
            let spec = self.service_spec_unchecked(m)?;
            if !spec.config.is_file() {
                return Err(format!(
                    "falta {}: la red no está completa; únete de nuevo",
                    spec.config.display()
                ));
            }
            Ok(spec)
        }
    }

    fn require_elevated() -> Result<(), String> {
        if is_elevated().map_err(|e| e.to_string())? {
            Ok(())
        } else {
            Err("administrar servicios requiere una consola de Administrador elevada".into())
        }
    }

    pub fn up(ctx: &Ctx, query: &str) -> Result<(), String> {
        require_elevated()?;
        let _lock = lock(ctx)?;
        up_locked(ctx, query)
    }

    /// `up` con el bloqueo de la raíz ya tomado por quien llama (`join --up`).
    fn up_locked(ctx: &Ctx, query: &str) -> Result<(), String> {
        let metas = read_metas(&ctx.public)?;
        let m = resolve(&metas, query)?;
        let spec = ctx.service_spec(m)?;
        check_trusted_binary(&spec.executable)
            .map_err(|e| format!("binario del servicio no confiable: {e}"))?;
        start_network_service(&spec).map_err(|e| format!("{}: {e}", spec.name))?;
        println!("red «{}» iniciada mediante scm", m.name);
        Ok(())
    }

    pub fn down(ctx: &Ctx, query: &str) -> Result<(), String> {
        require_elevated()?;
        let _lock = lock(ctx)?;
        let metas = read_metas(&ctx.public)?;
        let m = resolve(&metas, query)?;
        let spec = ctx.service_spec(m)?;
        let stopped = stop_network_service(&spec).map_err(|e| format!("{}: {e}", spec.name))?;
        println!(
            "red «{}» {}",
            m.name,
            if stopped {
                "detenida"
            } else {
                "ya estaba detenida"
            }
        );
        Ok(())
    }

    /// Bloqueo de la raíz de las redes, compartido por `join`, `up`, `down` y `leave`: el sistema lo suelta
    /// también si el proceso cae.
    fn lock(ctx: &Ctx) -> Result<std::fs::File, String> {
        std::fs::create_dir_all(&ctx.networks)
            .map_err(|e| format!("{}: {e}", ctx.networks.display()))?;
        let path = ctx.networks.join(".lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.lock()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(file)
    }

    fn pipe(id: &str, kind: &str) -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\atenea-{}-{kind}", short(id)))
    }

    /// Escribe el índice público: SYSTEM y Administradores escriben, los usuarios solo leen (ADR-017 §2).
    fn write_index(ctx: &Ctx, meta: &Meta) -> Result<(), String> {
        create_public_dir(&ctx.public).map_err(|e| format!("{}: {e}", ctx.public.display()))?;
        let path = ctx.public.join(format!("{}.json", meta.network_id));
        let body = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
        replace_public(&path, &body).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub struct JoinOpts {
        pub code: String,
        pub name: Option<String>,
        pub yes: bool,
        pub insecure_http: bool,
        pub stun_host: Option<String>,
        pub relay_ca: Option<PathBuf>,
        pub up: bool,
    }

    struct ConnectivityPlan {
        coordinator: enroll::Coordinator,
        relay_ca_pem: Option<String>,
    }

    /// Crea claves, `agent.toml`, registra el dispositivo y escribe el índice público.
    /// `accepted` en el error: el servidor ya aceptó la solicitud, no se debe deshacer en silencio.
    fn provision(
        ctx: &Ctx,
        code: &JoinCode,
        id: &str,
        url: &str,
        opts: &JoinOpts,
        port: u16,
        plan: Option<ConnectivityPlan>,
    ) -> Result<Meta, enroll::RegisterError> {
        let fail = |message: String| enroll::RegisterError {
            message,
            accepted: false,
        };
        let device = ctx.device_dir(id);
        std::fs::create_dir(&device).map_err(|e| fail(format!("{}: {e}", device.display())))?;
        // La CA verificada se guarda junto a las claves de esta red (una CA por red, sin ficheros compartidos).
        let coordinator = match plan {
            Some(ConnectivityPlan {
                coordinator,
                relay_ca_pem: Some(pem),
            }) => {
                let path = device.join("relay-ca.pem");
                std::fs::write(&path, pem.as_bytes())
                    .map_err(|e| fail(format!("{}: {e}", path.display())))?;
                Some(coordinator.with_relay_ca(path))
            }
            Some(ConnectivityPlan { coordinator, .. }) => Some(coordinator),
            None => None,
        };
        let (_, _, fingerprint) = issue::device_create(&device).map_err(fail)?;
        let interface = format!("atn_{}", short(id));
        enroll::pinned_config(
            url,
            &hex::encode(&code.network_id),
            &hex::encode(&code.root_anchor),
            &device,
            &device.join("agent.toml"),
            &interface,
            port,
            &pipe(id, "control"),
            &pipe(id, "status"),
            // BUILTIN\Users lee el estado, como en el cliente heredado.
            Some(545),
            opts.insecure_http,
            coordinator.as_ref(),
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
            service_backend: Some(Service::Scm),
        };
        write_index(ctx, &meta).map_err(accepted)?;
        Ok(meta)
    }

    pub fn join(
        ctx: &Ctx,
        opts: &JoinOpts,
        input: &mut dyn BufRead,
        interactive: bool,
    ) -> Result<(), String> {
        require_elevated()?;
        let code = JoinCode::decode(&opts.code).map_err(|e| format!("código no válido: {e}"))?;
        let info = code_info(&code);
        let url = if opts.insecure_http {
            code.url.replacen("https://", "http://", 1)
        } else {
            code.url.clone()
        };
        // Antes de confirmar y de tocar el disco: una opción inválida no deja nada. Las opciones explícitas
        // (`--stun-host`, `--relay-ca`) mandan sobre el bloque de conectividad del código (ADR-018).
        let (coordinator, ca_hash) = match (&opts.stun_host, &code.connectivity) {
            (Some(host), _) => (
                Some(enroll::Coordinator::new(host, opts.relay_ca.clone())?),
                None,
            ),
            (None, Some(c)) => (Some(enroll::Coordinator::from_code(c)), c.relay_ca_sha256),
            (None, None) => (None, None),
        };
        write_code_info(&info, &mut std::io::stdout().lock())?;
        if opts.stun_host.is_some() && code.connectivity.is_some() {
            println!(
                "Conectividad:   se usan --stun-host/--relay-ca; se ignora la que lleva el código."
            );
        }
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
        // La CA se pide DESPUÉS de confirmar y ANTES de tocar el disco o registrar nada: si su hash no coincide con
        // el del código, la unión se aborta sin dejar rastro ni en el servidor ni aquí.
        let relay_ca_pem = ca_hash
            .map(|hash| enroll::fetch_relay_ca(&url, &hash, opts.insecure_http))
            .transpose()?;
        let has_coordinator = coordinator.is_some();
        let plan = coordinator.map(|coordinator| ConnectivityPlan {
            coordinator,
            relay_ca_pem,
        });
        let _lock = lock(ctx)?;
        let metas = read_metas(&ctx.public)?;
        if let Some(m) = metas.iter().find(|m| m.network_id == id) {
            return Err(format!(
                "ya estás unido a la red «{}» ({id}); no se ha cambiado nada",
                m.name
            ));
        }
        if let Some(m) = metas.iter().find(|m| short(&m.network_id) == short(&id)) {
            return Err(format!(
                "el id corto {} ya lo usa la red «{}»; no se puede unir sin colisión de servicios y tuberías",
                short(&id),
                m.name
            ));
        }
        let used: Vec<u16> = metas.iter().map(|m| m.listen_port).collect();
        let port = allocate_port(&used, ctx.legacy_port())?;
        let net = ctx.net_dir(&id);
        std::fs::create_dir(&net).map_err(|e| format!("{}: {e}", net.display()))?;
        match provision(ctx, &code, &id, &url, opts, port, plan) {
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
                if !has_coordinator {
                    println!(
                        "AVISO: ni el código ni --stun-host llevan conectividad: no se escribe [coordinator]; este equipo se sincronizará pero no hará rendezvous ni usará el relay (sin camino directo)."
                    );
                }
                if opts.up {
                    up_locked(ctx, &id).map_err(|e| {
                        format!(
                            "unido a la red (el servidor ya aceptó la solicitud), pero no se pudo arrancar: {e}. \
                             Arráncala con `atenea network up {}` desde una consola de Administrador",
                            short(&id)
                        )
                    })?;
                    println!(
                        "Si el dispositivo está pendiente de aprobación, la red se conectará cuando un administrador apruebe su huella."
                    );
                } else {
                    println!(
                        "Siguiente paso: atenea network up {} (consola de Administrador)",
                        short(&id)
                    );
                }
                Ok(())
            }
            Err(e) if !e.accepted => {
                std::fs::remove_dir_all(&net).map_err(|x| format!("{}: {x}", net.display()))?;
                Err(format!("{} (no se ha dejado nada en el disco)", e.message))
            }
            Err(e) => Err(format!(
                "{}. El servidor YA aceptó la solicitud: se conserva {} (claves nuevas del dispositivo); \
                 si no puedes completarla, usa `atenea network leave {id}` para revocar el dispositivo",
                e.message,
                net.display()
            )),
        }
    }

    pub fn leave(
        ctx: &Ctx,
        query: &str,
        yes: bool,
        offline: bool,
        input: &mut dyn BufRead,
        interactive: bool,
    ) -> Result<(), String> {
        require_elevated()?;
        let metas = read_metas(&ctx.public)?;
        let m = resolve(&metas, query)?;
        println!(
            "Vas a salir de la red «{}» ({}). Se revocará el dispositivo en el servidor (salvo --offline), se detendrá su servicio, se limpiarán sus rutas y DNS y se BORRARÁN las claves de este dispositivo en esa red: es irreversible.",
            m.name, m.network_id
        );
        confirm("¿Salir de la red?", yes, interactive, input)?;
        let _lock = lock(ctx)?;
        let metas = read_metas(&ctx.public)?;
        let m = resolve(&metas, query)?;
        let net = ctx.net_dir(&m.network_id);
        let found =
            std::fs::symlink_metadata(&net).map_err(|e| format!("{}: {e}", net.display()))?;
        if !found.is_dir() || found.file_type().is_symlink() {
            return Err(format!(
                "{}: no es un directorio; no se borra",
                net.display()
            ));
        }
        if offline {
            eprintln!("{}", offline_warning(m));
        } else {
            enroll::leave(&m.url, &m.device_id, &ctx.device_dir(&m.network_id))
                .map_err(|e| format!("{e}. No se ha parado ni borrado nada. Reintenta con el servidor disponible; --offline solo borra lo local y requiere revocación administrativa por huella."))?;
        }
        let cleanup = || -> Result<(), String> {
            let spec = ctx.service_spec_unchecked(m)?;
            let name = spec.name.clone();
            let svc = |e: std::io::Error| format!("{name}: {e}");
            stop_network_service(&spec).map_err(svc)?;
            let config = ctx.device_dir(&m.network_id).join("agent.toml");
            if config.is_file() {
                check_trusted_binary(&spec.executable)
                    .map_err(|e| format!("binario del servicio no confiable: {e}"))?;
                let status = Command::new(&spec.executable)
                    .args(["cleanup", "--config"])
                    .arg(&config)
                    .status()
                    .map_err(|e| format!("{}: {e}", spec.executable.display()))?;
                if !status.success() {
                    return Err(format!(
                        "la limpieza del agente falló ({status}); no se han borrado las claves"
                    ));
                }
            }
            delete_network_service(&spec).map_err(svc)?;
            std::fs::remove_dir_all(&net).map_err(|e| format!("{}: {e}", net.display()))?;
            // El índice se retira el último: si algo falla antes, `leave` se puede repetir.
            let index = ctx.public.join(format!("{}.json", m.network_id));
            std::fs::remove_file(&index).map_err(|e| format!("{}: {e}", index.display()))
        };
        cleanup().map_err(|e| {
            if offline {
                e
            } else {
                format!(
                    "El dispositivo YA está revocado en el servidor; la limpieza local falló: {e}"
                )
            }
        })?;
        println!(
            "Has salido de la red «{}»; sus claves se han borrado.",
            m.name
        );
        if !offline {
            println!("Dispositivo revocado en el servidor.");
        }
        Ok(())
    }

    /// `AteneaNetwork-<id8>`: el nombre de la red (entrada no confiable) nunca entra en él.
    fn service_name(id: &str) -> String {
        format!("AteneaNetwork-{}", short(id))
    }

    /// El SCM dice si el servicio corre; solo entonces se pregunta a su tubería de estado.
    fn probe(m: &Meta) -> State {
        match service_status(&service_name(&m.network_id)) {
            Ok(ServiceStatus::Missing | ServiceStatus::Stopped) => State::Stopped,
            Ok(_) => match crate::ask(&pipe(&m.network_id, "status"), "status") {
                Ok(text) => state_from_status(&text),
                Err(e) => State::Unknown(e),
            },
            Err(e) => State::Unknown(format!("SCM: {e}")),
        }
    }

    pub fn list_json(ctx: &Ctx) -> Result<serde_json::Value, String> {
        Ok(read_metas(&ctx.public)?
            .iter()
            .map(|m| {
                let state = probe(m);
                let mut v = serde_json::to_value(m).expect("serializable");
                v["state"] = state.code().into();
                v["backend"] = "scm".into();
                if let State::Conflict(d) | State::Unknown(d) = &state {
                    v["detail"] = d.clone().into();
                }
                v
            })
            .collect())
    }

    pub fn list(ctx: &Ctx, json: bool) -> Result<(), String> {
        if json {
            println!("{}", list_json(ctx)?);
            return Ok(());
        }
        let metas = read_metas(&ctx.public)?;
        if metas.is_empty() {
            println!("No hay redes unidas.");
        }
        for m in &metas {
            println!(
                "{}  {}  {}  {}  {}  {}  scm",
                m.name,
                short(&m.network_id),
                m.server,
                m.interface,
                m.listen_port,
                probe(m).text()
            );
        }
        Ok(())
    }

    /// Windows no migra la instalación heredada (ADR-017 §10): solo informa de que existe.
    pub fn legacy(json: bool, output: &mut dyn Write) -> Result<(), String> {
        let present = !matches!(
            service_status(LEGACY_SERVICE).map_err(|e| format!("SCM: {e}"))?,
            ServiceStatus::Missing
        );
        let text = if json {
            serde_json::json!({ "present": present }).to_string()
        } else if present {
            "Hay una instalación heredada (servicio AteneaAgent). Windows no la migra: desinstálala, reinstala y únete con `atenea network join`".into()
        } else {
            "No hay instalación heredada".to_string()
        };
        writeln!(output, "{text}").map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "00000000-0000-4000-8000-00001111aaaa";
    const B: &str = "00000000-0000-4000-8000-00002222bbbb";

    fn meta(id: &str, name: &str, created: u64) -> String {
        serde_json::json!({
            "network_id": id, "name": name, "server": "control.example.test", "url": "https://control.example.test",
            "root_anchor_fingerprint": "f", "device_id": "d", "device_fingerprint": "p",
            "interface": format!("atn_{}", &id[28..]), "listen_port": 51820, "created": created,
            "join_status": "aprobada"
        })
        .to_string()
    }

    #[test]
    fn the_public_index_lists_valid_networks_in_creation_order_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("atenea-public-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{B}.json")), meta(B, "segunda", 2)).unwrap();
        std::fs::write(dir.join(format!("{A}.json")), meta(A, "primera", 1)).unwrap();
        // Nombre y contenido discrepantes, JSON roto y ficheros ajenos: se omiten sin fallar.
        std::fs::write(
            dir.join("00000000-0000-4000-8000-00003333cccc.json"),
            meta(A, "impostora", 0),
        )
        .unwrap();
        std::fs::write(dir.join("00000000-0000-4000-8000-00004444dddd.json"), "{").unwrap();
        std::fs::write(dir.join("notas.txt"), "x").unwrap();
        let names: Vec<_> = read_metas(&dir)
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, ["primera", "segunda"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(read_metas(&dir).unwrap().is_empty());
    }

    #[test]
    fn the_udp_port_skips_registered_networks_the_legacy_install_and_busy_ports() {
        let first = allocate_port(&[], None).unwrap();
        assert!(first >= 51820);
        assert_ne!(allocate_port(&[first], None).unwrap(), first);
        assert_ne!(allocate_port(&[], Some(first)).unwrap(), first);
        // Un puerto ocupado por otro proceso tampoco se reutiliza.
        let held = std::net::UdpSocket::bind(("0.0.0.0", first)).unwrap();
        assert_ne!(allocate_port(&[], None).unwrap(), first);
        drop(held);
    }

    #[test]
    fn the_legacy_port_comes_from_its_listen_address() {
        assert_eq!(legacy_port("listen = \"0.0.0.0:51830\"\n"), Some(51830));
        assert_eq!(legacy_port("listen = \"no\"\n"), None);
        assert_eq!(legacy_port("="), None);
    }
}
