// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Nombres de dispositivo (ADR-012): el del sistema y los que reparte el control plane.
//!
//! Son metadato de visualización: no se usan para identificar, autorizar, resolver ni enrutar.
//! Sin dependencias internas: el CLI incluye este fichero con `#[path]`.
#![allow(dead_code)] // el CLI solo usa `system_name`

pub const MAX_CHARS: usize = 64;

/// Controles (Cc) y formato bidireccional: permiten disfrazar o reordenar el texto mostrado.
fn forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Nombre recibido de otro dispositivo o del control plane: o se acepta (recortado) o se rechaza.
pub fn accept(raw: &str) -> Option<String> {
    let name = raw.trim();
    ((1..=MAX_CHARS).contains(&name.chars().count()) && !name.chars().any(forbidden))
        .then(|| name.to_string())
}

/// Nombre propio leído del sistema: no se rechaza, se limpia (sin controles y a `MAX_CHARS`).
pub fn clean_own(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !forbidden(*c)).collect();
    accept(&cleaned.trim().chars().take(MAX_CHARS).collect::<String>())
}

/// Nombre del equipo tal como lo ve el usuario (macOS: «Nombre» de Ajustes; resto: el del host).
pub fn system_name() -> Option<String> {
    clean_own(&read_system_name()?)
}

#[cfg(target_os = "linux")]
fn read_system_name() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname").ok()
}

#[cfg(target_os = "macos")]
fn read_system_name() -> Option<String> {
    let run = |program: &str, args: &[&str]| {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).ok())
            .flatten()
    };
    run("/usr/sbin/scutil", &["--get", "ComputerName"]).or_else(|| run("/bin/hostname", &[]))
}

#[cfg(windows)]
fn read_system_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read_system_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_human_names_and_rejects_anything_that_can_disguise_text() {
        for ok in [
            "MacBook Air de Guillem",
            "  portátil  ",
            "日本語のPC",
            "pc 💻",
            &"a".repeat(64),
        ] {
            assert_eq!(accept(ok).as_deref(), Some(ok.trim()));
        }
        for bad in [
            "",
            "   ",
            "a\nb",
            "a\u{0}b",
            "a\u{7f}b",
            "a\u{85}b",
            "a\u{202e}b",
            "a\u{2066}b",
        ] {
            assert_eq!(accept(bad), None, "{bad:?}");
        }
        assert_eq!(accept(&"a".repeat(65)), None);
        // 64 caracteres no son 64 bytes: se cuentan caracteres.
        assert!(accept(&"ñ".repeat(64)).is_some());
    }

    #[test]
    fn own_name_is_cleaned_instead_of_rejected() {
        assert_eq!(clean_own("host\n").as_deref(), Some("host"));
        assert_eq!(clean_own("a\u{202e}b").as_deref(), Some("ab"));
        assert_eq!(
            clean_own(&"x".repeat(100)).map(|n| n.chars().count()),
            Some(64)
        );
        assert_eq!(clean_own("\n\t"), None);
    }
}
