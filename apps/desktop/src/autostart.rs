// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! «Abrir al iniciar sesión»: el estado es la existencia de un fichero de arranque del propio usuario
//! (LaunchAgent en macOS, `.desktop` XDG en Linux). Sin privilegios, sin servicio residente y sin
//! preferencia aparte que pueda desincronizarse. Windows no está soportado.

use std::path::{Path, PathBuf};

const LABEL: &str = "one.atenea.desktop";

/// Fichero de arranque del usuario; `None` donde no hay mecanismo soportado.
fn entry_path(home: &Path, xdg_config: Option<&Path>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(home.join(format!("Library/LaunchAgents/{LABEL}.plist")))
    } else if cfg!(target_os = "linux") {
        let base = xdg_config
            .filter(|p| p.is_absolute())
            .map_or_else(|| home.join(".config"), Path::to_path_buf);
        Some(base.join(format!("autostart/{LABEL}.desktop")))
    } else {
        None
    }
}

/// `None`: no disponible en este sistema operativo.
pub fn get(home: &Path, xdg_config: Option<&Path>) -> Result<Option<bool>, String> {
    let Some(path) = entry_path(home, xdg_config) else {
        return Ok(None);
    };
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(true)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Some(false)),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set(home: &Path, xdg_config: Option<&Path>, exe: &Path, on: bool) -> Result<(), String> {
    let path = entry_path(home, xdg_config)
        .ok_or("«Abrir al iniciar sesión» no está disponible en este sistema operativo")?;
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let body = render(exe)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, body).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

/// El ejecutable debe ser una ruta absoluta sin caracteres que cambien el sentido de la línea de
/// arranque (comillas, `\`, `%`, `$`, comilla invertida, saltos o controles).
fn checked_exe(exe: &Path) -> Result<&str, String> {
    let text = exe
        .to_str()
        .filter(|t| exe.is_absolute() && !t.is_empty())
        .ok_or("ruta del ejecutable no válida")?;
    if text.chars().any(|c| {
        c.is_control() || matches!(c, '"' | '\\' | '%' | '$' | '`' | '<' | '>' | '&' | '\'')
    }) {
        return Err("la ruta del ejecutable contiene caracteres no admitidos".into());
    }
    Ok(text)
}

fn render(exe: &Path) -> Result<String, String> {
    let exe = checked_exe(exe)?;
    Ok(if cfg!(target_os = "macos") {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n  <key>Label</key><string>{LABEL}</string>\n  \
<key>ProgramArguments</key><array><string>{exe}</string></array>\n  \
<key>RunAtLoad</key><true/>\n</dict></plist>\n"
        )
    } else {
        format!(
            "[Desktop Entry]\nType=Application\nName=Atenea One\nExec=\"{exe}\"\nTerminal=false\n\
X-GNOME-Autostart-enabled=true\n"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("atenea-autostart-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn enable_disable_round_trips_and_is_idempotent() {
        let h = home("round");
        let exe = Path::new("/Applications/Atenea One.app/Contents/MacOS/atenea-desktop");
        assert_eq!(get(&h, None), Ok(Some(false)));
        set(&h, None, exe, false).unwrap(); // desactivar sin fichero no falla
        set(&h, None, exe, true).unwrap();
        set(&h, None, exe, true).unwrap(); // reactivar reescribe sin error
        assert_eq!(get(&h, None), Ok(Some(true)));
        #[cfg(target_os = "macos")]
        assert!(
            std::process::Command::new("/usr/bin/plutil")
                .arg("-lint")
                .arg(entry_path(&h, None).unwrap())
                .output()
                .unwrap()
                .status
                .success(),
            "plist inválido"
        );
        let written = std::fs::read_to_string(entry_path(&h, None).unwrap()).unwrap();
        assert!(written.contains("Atenea One.app/Contents/MacOS/atenea-desktop"));
        assert!(written.contains("RunAtLoad") || written.contains("Exec=\""));
        set(&h, None, exe, false).unwrap();
        assert_eq!(get(&h, None), Ok(Some(false)));
        let _ = std::fs::remove_dir_all(h);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn enabling_replaces_a_stale_entry_with_the_current_executable() {
        let h = home("stale");
        let path = entry_path(&h, None).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "antiguo /otra/ruta").unwrap();
        set(&h, None, Path::new("/opt/atenea/atenea-desktop"), true).unwrap();
        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("/opt/atenea/atenea-desktop") && !written.contains("antiguo"));
        let _ = std::fs::remove_dir_all(h);
    }

    #[test]
    fn executable_path_must_be_absolute_and_free_of_syntax_characters() {
        for bad in [
            "relativa/atenea",
            "",
            "/a\"b",
            "/a\\b",
            "/a%u",
            "/a$HOME",
            "/a`id`",
            "/a\nb",
            "/a<b",
            "/a&b",
            "/a'b",
        ] {
            assert!(checked_exe(Path::new(bad)).is_err(), "{bad:?}");
        }
        assert!(checked_exe(Path::new("/Applications/Atenea One.app/Contents/MacOS/x")).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn xdg_config_home_is_honoured_only_when_absolute() {
        let h = home("xdg");
        let rel = entry_path(&h, Some(Path::new("relativo"))).unwrap();
        assert!(rel.starts_with(h.join(".config")));
        let abs = entry_path(&h, Some(Path::new("/tmp/xdg"))).unwrap();
        assert_eq!(
            abs,
            Path::new("/tmp/xdg/autostart/one.atenea.desktop.desktop")
        );
        let _ = std::fs::remove_dir_all(h);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn unsupported_systems_report_none_and_refuse_to_set() {
        let h = home("unsupported");
        assert_eq!(get(&h, None), Ok(None));
        assert!(set(&h, None, Path::new("/x"), true).is_err());
    }
}
