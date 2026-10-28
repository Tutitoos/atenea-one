// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Redes (ADR-013 fase 4): lectura sin privilegios y operaciones que cambian el sistema con elevación
//! del sistema operativo (opción A: un diálogo de administrador por operación, sin servicio residente).
//!
//! La app no implementa ninguna operación: ejecuta el CLI `atenea network …`. El frontend solo puede
//! pedir las operaciones de [`Op`] con argumentos que este módulo valida antes de construir la orden,
//! de modo que ningún texto del usuario llega al intérprete de órdenes sin haber pasado por ahí.

use serde_json::Value;
use std::process::{Command, Stdio};

/// Operaciones que cambian el sistema. Cada una exige la autorización del sistema operativo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Join,
    Up,
    Down,
    Leave,
}

impl Op {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "join" => Ok(Self::Join),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "leave" => Ok(Self::Leave),
            _ => Err("operación no permitida".into()),
        }
    }
}

/// Binario del CLI: el que instala el paquete. `ATENEA_CLI` lo sustituye (desarrollo y pruebas).
pub fn cli_path() -> String {
    std::env::var("ATENEA_CLI").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/Library/Atenea/bin/atenea".into()
        } else {
            "/usr/bin/atenea".into()
        }
    })
}

/// Uuid en minúsculas con la forma canónica 8-4-4-4-12 (el servidor lo genera así y el CLI lo exige).
pub fn valid_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// Id corto de la red (últimos 8 hex): nombra su directorio de ejecución, igual que el CLI.
pub fn short_id(uuid: &str) -> &str {
    &uuid[28..]
}

/// Normaliza un código pegado (espacios, saltos de línea, minúsculas) y exige la forma del formato:
/// prefijo `ATENEA1-`, alfabeto base32 `A–Z2–7` y guiones, máximo 512 caracteres. El CLI lo vuelve a
/// validar entero; esto existe para que nada fuera de ese alfabeto llegue a una orden de shell.
pub fn normalize_code(raw: &str) -> Result<String, String> {
    let code: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let body = code
        .strip_prefix("ATENEA1-")
        .ok_or("el código debe empezar por ATENEA1-")?;
    if code.len() > 512 {
        return Err("el código es demasiado largo".into());
    }
    if body.is_empty()
        || !body
            .bytes()
            .all(|b| b == b'-' || b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
    {
        return Err("el código tiene caracteres que no son de un código de red".into());
    }
    Ok(code)
}

/// Grupo primario del usuario actual: se pasa a `join --status-gid` para que el socket de estado de la
/// red nueva sea legible sin privilegios (si no, la app vería todas las redes como «desconocida»).
pub fn primary_gid() -> Result<u32, String> {
    let out = Command::new("/usr/bin/id")
        .arg("-g")
        .output()
        .map_err(|e| format!("no se pudo obtener el grupo del usuario: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .map_err(|_| "no se pudo obtener el grupo del usuario".to_string())
}

/// Orden de shell que se ejecuta con privilegios. Todos los fragmentos variables ya están validados
/// (`valid_uuid`, `normalize_code`, un número), así que no contienen comillas ni metacaracteres.
pub fn script(
    cli: &str,
    op: Op,
    network: Option<&str>,
    code: Option<&str>,
    gid: u32,
) -> Result<String, String> {
    if !cli
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    {
        return Err("ruta del CLI no válida".into());
    }
    let net = |n: Option<&str>| -> Result<String, String> {
        match n {
            Some(n) if valid_uuid(n) => Ok(n.to_string()),
            _ => Err("identificador de red no válido".into()),
        }
    };
    Ok(match op {
        Op::Join => {
            let code = normalize_code(code.ok_or("falta el código")?)?;
            // Tras unirse, la red se inicia en el mismo diálogo de autorización. El id sale del propio
            // código (lo decodifica `inspect` en el frontend); aquí se exige que el llamante lo dé.
            let id = net(network)?;
            format!("{cli} network join {code} --yes --status-gid {gid} && {cli} network up {id}")
        }
        Op::Up => format!("{cli} network up {}", net(network)?),
        Op::Down => format!("{cli} network down {}", net(network)?),
        Op::Leave => format!("{cli} network leave {} --yes", net(network)?),
    })
}

#[cfg(target_os = "macos")]
fn elevate(script: &str, prompt: &str) -> Result<String, String> {
    // `script` y `prompt` no llevan comillas ni barras invertidas (validado arriba / constantes).
    let apple = format!(
        "do shell script \"{script}\" with administrator privileges with prompt \"{prompt}\""
    );
    run("/usr/bin/osascript", &["-e", &apple])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn elevate(script: &str, _prompt: &str) -> Result<String, String> {
    run("/usr/bin/pkexec", &["/bin/sh", "-c", script])
}

#[cfg(windows)]
fn elevate(_script: &str, _prompt: &str) -> Result<String, String> {
    Err("Unirse a redes todavía no está soportado en Windows".into())
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    // El usuario cerró el diálogo de administrador: macOS (-128) y polkit (126 «no autorizado»/127).
    if stderr.contains("User canceled") || matches!(out.status.code(), Some(126 | 127)) {
        return Err("Cancelado: no se ha cambiado nada".into());
    }
    Err(if stderr.is_empty() { stdout } else { stderr })
}

/// `atenea network list --json` sin privilegios. Sin red unida, o en Windows, es una lista vacía.
pub fn list() -> Result<String, String> {
    if cfg!(windows) {
        return Ok("[]".into());
    }
    let cli = cli_path();
    if !std::path::Path::new(&cli).exists() {
        return Ok("[]".into());
    }
    let text = run(&cli, &["network", "list", "--json"])?;
    // Rechaza cualquier salida que no sea un array JSON antes de entregarla al frontend.
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Array(_)) => Ok(text),
        _ => Err("`atenea network list` devolvió una respuesta inesperada".into()),
    }
}

/// Decodifica un código sin privilegios, sin disco y sin red (`atenea network inspect`).
pub fn inspect(raw: &str) -> Result<String, String> {
    let code = normalize_code(raw)?;
    let text = run(&cli_path(), &["network", "inspect", &code, "--json"])?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(_)) => Ok(text),
        _ => Err("`atenea network inspect` devolvió una respuesta inesperada".into()),
    }
}

/// Ejecuta una operación con el diálogo de autorización del sistema. En `join` el id de la red sale del
/// propio código (lo decodifica `inspect`), nunca del frontend: el `up` posterior no puede apuntar a otra red.
pub fn apply(op: &str, network: Option<&str>, code: Option<&str>) -> Result<String, String> {
    let op = Op::parse(op)?;
    let (gid, network) = if op == Op::Join {
        let info: Value = serde_json::from_str(&inspect(code.ok_or("falta el código")?)?)
            .map_err(|e| e.to_string())?;
        let id = info["network_id"]
            .as_str()
            .filter(|id| valid_uuid(id))
            .ok_or("el código no trae un id de red válido")?;
        (primary_gid()?, Some(id.to_string()))
    } else {
        (0, network.map(str::to_string))
    };
    let script = script(&cli_path(), op, network.as_deref(), code, gid)?;
    let prompt = match op {
        Op::Join => "Atenea One quiere unir este equipo a una red.",
        Op::Up => "Atenea One quiere conectar este equipo a una red.",
        Op::Down => "Atenea One quiere desconectar este equipo de una red.",
        Op::Leave => "Atenea One quiere sacar este equipo de una red y borrar sus claves.",
    };
    elevate(&script, prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0199ab12-3456-7def-8abc-0123456789ab";
    const CLI: &str = "/Library/Atenea/bin/atenea";

    #[test]
    fn uuid_must_be_canonical_lowercase() {
        assert!(valid_uuid(ID));
        for bad in [
            "",
            "0199AB12-3456-7def-8abc-0123456789ab",
            "0199ab123456-7def-8abc-0123456789ab",
            "0199ab12-3456-7def-8abc-0123456789a;",
            "0199ab12-3456-7def-8abc-0123456789abc",
            "$(id)",
        ] {
            assert!(!valid_uuid(bad), "{bad}");
        }
        assert_eq!(short_id(ID), "456789ab");
    }

    #[test]
    fn pasted_code_is_normalized_and_only_the_format_alphabet_passes() {
        let ok = normalize_code("  atenea1-abcd ef-gh23\n-7x ").unwrap();
        assert_eq!(ok, "ATENEA1-ABCDEF-GH23-7X");
        assert!(normalize_code("ATENEA1-").is_err());
        assert!(normalize_code("ABCDEF-GHIJ").is_err());
        // Base32 no tiene 0, 1, 8 ni 9: un carácter fuera del alfabeto es siempre un error.
        assert!(normalize_code("ATENEA1-ABC1").is_err());
        for bad in [
            "ATENEA1-AB;rm -rf",
            "ATENEA1-AB\"CD",
            "ATENEA1-AB$(id)",
            "ATENEA1-AB`id`",
            "ATENEA1-AB&&id",
            "ATENEA1-AB\\CD",
        ] {
            assert!(normalize_code(bad).is_err(), "{bad}");
        }
        assert!(normalize_code(&format!("ATENEA1-{}", "A".repeat(512))).is_err());
        assert!(normalize_code(&format!("ATENEA1-{}", "A".repeat(504))).is_ok());
    }

    #[test]
    fn privileged_scripts_are_exactly_the_allowed_cli_calls() {
        let code = "ATENEA1-ABCDEF-GH23";
        assert_eq!(
            script(CLI, Op::Join, Some(ID), Some(code), 20).unwrap(),
            format!("{CLI} network join {code} --yes --status-gid 20 && {CLI} network up {ID}")
        );
        assert_eq!(
            script(CLI, Op::Up, Some(ID), None, 0).unwrap(),
            format!("{CLI} network up {ID}")
        );
        assert_eq!(
            script(CLI, Op::Down, Some(ID), None, 0).unwrap(),
            format!("{CLI} network down {ID}")
        );
        assert_eq!(
            script(CLI, Op::Leave, Some(ID), None, 0).unwrap(),
            format!("{CLI} network leave {ID} --yes")
        );
    }

    #[test]
    fn no_unvalidated_text_reaches_the_privileged_script() {
        for bad_id in [
            None,
            Some(""),
            Some("0199ab12; id"),
            Some("sudo"),
            Some("--yes"),
        ] {
            for op in [Op::Up, Op::Down, Op::Leave, Op::Join] {
                assert!(
                    script(CLI, op, bad_id, Some("ATENEA1-ABCD"), 20).is_err(),
                    "{op:?} {bad_id:?}"
                );
            }
        }
        assert!(script(CLI, Op::Join, Some(ID), None, 20).is_err());
        assert!(script(CLI, Op::Join, Some(ID), Some("ATENEA1-AB; id"), 20).is_err());
        assert!(script("/x; id", Op::Up, Some(ID), None, 0).is_err());
        assert!(script("/x\"y", Op::Up, Some(ID), None, 0).is_err());
    }

    #[test]
    fn only_listed_operations_exist() {
        assert_eq!(Op::parse("join"), Ok(Op::Join));
        for bad in ["", "JOIN", "shell", "list", "up; id", "leave "] {
            assert!(Op::parse(bad).is_err(), "{bad}");
        }
    }
}
