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

#[cfg(windows)]
pub use system::*;

#[cfg(windows)]
mod system {
    use super::read_metas;
    use crate::network_common::{Meta, State, short, state_from_status};
    use atenea_windows::{ServiceStatus, service_status};
    use std::io::Write;
    use std::path::PathBuf;

    /// Servicio heredado de ADR-008, anterior a las redes por instancia.
    const LEGACY_SERVICE: &str = "AteneaAgent";

    pub struct Ctx {
        public: PathBuf,
    }

    impl Ctx {
        pub fn new() -> Result<Self, String> {
            let public = match std::env::var_os("ATENEA_PUBLIC_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => PathBuf::from(
                    std::env::var_os("ProgramData").ok_or("falta la variable ProgramData")?,
                )
                .join("Atenea")
                .join("public"),
            };
            Ok(Self { public })
        }
    }

    /// `AteneaNetwork-<id8>`: el nombre de la red (entrada no confiable) nunca entra en él.
    fn service_name(id: &str) -> String {
        format!("AteneaNetwork-{}", short(id))
    }

    fn status_pipe(id: &str) -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\atenea-{}-status", short(id)))
    }

    /// El SCM dice si el servicio corre; solo entonces se pregunta a su tubería de estado.
    fn probe(m: &Meta) -> State {
        match service_status(&service_name(&m.network_id)) {
            Ok(ServiceStatus::Missing | ServiceStatus::Stopped) => State::Stopped,
            Ok(_) => match crate::ask(&status_pipe(&m.network_id), "status") {
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
}
