// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Servicios por red. No se reutiliza ni se modifica la instancia heredada.
use clap::ValueEnum;
#[cfg(unix)]
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Backend {
    #[default]
    Auto,
    Process,
    System,
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Component, Path, PathBuf};
    use std::process::Command;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Service {
        Process,
        Systemd,
        Launchd,
    }

    impl Service {
        pub fn name(self) -> &'static str {
            match self {
                Self::Process => "process",
                Self::Systemd => "systemd",
                Self::Launchd => "launchd",
            }
        }
    }

    pub fn choose(mode: Backend, saved: Option<Service>) -> Service {
        match mode {
            Backend::Process => Service::Process,
            Backend::System => {
                if cfg!(target_os = "macos") {
                    Service::Launchd
                } else {
                    Service::Systemd
                }
            }
            Backend::Auto => saved.unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    Service::Launchd
                } else if std::fs::read_to_string("/proc/1/comm")
                    .is_ok_and(|s| s.trim() == "systemd")
                {
                    Service::Systemd
                } else {
                    Service::Process
                }
            }),
        }
    }

    fn validate_id(id: &str) -> Result<(), String> {
        if id.len() != 36
            || !id.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            })
        {
            return Err("id de red: se exige UUID canónico en minúsculas".into());
        }
        Ok(())
    }

    pub fn label(id: &str) -> Result<String, String> {
        validate_id(id)?;
        Ok(format!("one.atenea.network.{}", &id[28..]))
    }
    fn unit(id: &str) -> Result<String, String> {
        validate_id(id)?;
        Ok(format!("atenea-network@{id}.service"))
    }

    /// Valida la cadena completa, no solo el fichero: nunca publica una orden privilegiada que
    /// apunte a una configuración reemplazable por un usuario local.
    pub fn trusted(path: &Path) -> Result<(), String> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("el servicio exige una ruta absoluta sin . ni ..".into());
        }
        for p in path.ancestors() {
            let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
            if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                return Err(format!(
                    "{}: ruta no confiable para un servicio (root, sin enlaces ni escritura de grupo/otros)",
                    p.display()
                ));
            }
        }
        Ok(())
    }

    fn root_required() -> Result<(), String> {
        let out = Command::new("/usr/bin/id")
            .arg("-u")
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() || out.stdout != b"0\n" {
            return Err("administrar servicios requiere root: ejecuta con sudo".into());
        }
        Ok(())
    }
    fn command(program: &str, args: &[&str]) -> Result<(), String> {
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("{program}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{program} {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
    fn systemctl() -> &'static str {
        if Path::new("/usr/bin/systemctl").exists() {
            "/usr/bin/systemctl"
        } else {
            "/bin/systemctl"
        }
    }
    fn systemd_quote(path: &Path) -> Result<String, String> {
        let text = path.to_str().ok_or("ruta no UTF-8")?;
        if text.chars().any(char::is_control) {
            return Err("ruta con caracteres de control".into());
        }
        Ok(format!(
            "\"{}\"",
            text.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('%', "%%")
                .replace('$', "$$")
        ))
    }
    pub fn dropin(root: &Path, id: &str) -> Result<String, String> {
        validate_id(id)?;
        let config = systemd_quote(&root.join(id).join("device/agent.toml"))?;
        let literal = config.replace("$$", "$");
        let writable = systemd_quote(&root.join(id).join("device"))?.replace("$$", "$");
        Ok(format!(
            "# Copyright 2026 Tutitoos\n# SPDX-License-Identifier: Apache-2.0\n[Unit]\nConditionPathExists=\nConditionPathExists={literal}\n[Service]\nReadWritePaths={writable}\nRuntimeDirectory=\nRuntimeDirectory=atenea/{}\nRuntimeDirectoryPreserve=no\nExecStart=\nExecStart=/usr/lib/atenea/network-agent run {id} {config}\nExecStopPost=\nExecStopPost=/usr/lib/atenea/network-agent cleanup {id} {config}\n",
            &id[28..]
        ))
    }
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    pub fn plist(root: &Path, id: &str) -> Result<String, String> {
        let label = label(id)?;
        let config = root.join(id).join("device/agent.toml");
        let root = root.to_str().ok_or("ruta no UTF-8")?;
        if root.chars().any(char::is_control) {
            return Err("ruta con caracteres de control".into());
        }
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array><string>/Library/Atenea/bin/atenea-launchd.sh</string><string>{id}</string><string>{root}</string></array>
<key>UserName</key><string>root</string>
<key>GroupName</key><string>wheel</string>
<key>Umask</key><integer>63</integer>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>PathState</key><dict><key>{config}</key><true/></dict></dict>
<key>ProcessType</key><string>Background</string>
<key>ThrottleInterval</key><integer>10</integer>
<key>StandardOutPath</key><string>{log}</string>
<key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"#,
            root = xml(root),
            config = xml(config.to_str().ok_or("ruta no UTF-8")?),
            log = xml(config
                .with_file_name("agent.log")
                .to_str()
                .ok_or("ruta no UTF-8")?)
        ))
    }
    fn write_managed(path: &Path, text: &str, marker: &str) -> Result<(), String> {
        let parent = path.parent().ok_or("falta directorio padre")?;
        trusted(parent)?;
        if path.exists() {
            trusted(path)?;
            let old = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            if !old.contains(marker) {
                return Err(format!(
                    "{}: fichero ajeno, no se sobrescribe",
                    path.display()
                ));
            }
        }
        let tmp = parent.join(format!(".atenea-{}", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        let result = (|| {
            f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
                .map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }
    fn dropin_path(id: &str) -> Result<PathBuf, String> {
        Ok(Path::new("/etc/systemd/system")
            .join(format!("{}.d", unit(id)?))
            .join("paths.conf"))
    }
    fn plist_path(id: &str) -> Result<PathBuf, String> {
        Ok(Path::new("/Library/LaunchDaemons").join(format!("{}.plist", label(id)?)))
    }
    pub fn start(service: Service, root: &Path, id: &str) -> Result<(), String> {
        root_required()?;
        trusted(&root.join(id).join("device/agent.toml"))?;
        match service {
            Service::Process => Err("servicio proceso no tiene unidad del sistema".into()),
            Service::Systemd => {
                let target = dropin_path(id)?;
                let parent = target.parent().expect("padre");
                if !parent.exists() {
                    trusted(parent.parent().expect("padre"))?;
                    std::fs::create_dir(parent).map_err(|e| e.to_string())?;
                }
                write_managed(&target, &dropin(root, id)?, "/usr/lib/atenea/network-agent")?;
                command(systemctl(), &["daemon-reload"])?;
                command(systemctl(), &["enable", "--now", &unit(id)?])
            }
            Service::Launchd => {
                let label = label(id)?;
                let target = plist_path(id)?;
                write_managed(
                    &target,
                    &plist(root, id)?,
                    &format!("<string>{label}</string>"),
                )?;
                // La creación se hace siendo root; wheel es el grupo 0 de Darwin.
                std::os::unix::fs::chown(&target, Some(0), Some(0)).map_err(|e| e.to_string())?;
                command("/bin/launchctl", &["enable", &format!("system/{label}")])?;
                if !Command::new("/bin/launchctl")
                    .args(["print", &format!("system/{label}")])
                    .output()
                    .is_ok_and(|o| o.status.success())
                {
                    command(
                        "/bin/launchctl",
                        &[
                            "bootstrap",
                            "system",
                            target.to_str().ok_or("ruta no UTF-8")?,
                        ],
                    )?;
                }
                Ok(())
            }
        }
    }
    pub fn stop(service: Service, id: &str) -> Result<(), String> {
        root_required()?;
        match service {
            Service::Process => Ok(()),
            Service::Systemd => command(systemctl(), &["disable", "--now", &unit(id)?]),
            Service::Launchd => {
                let service = format!("system/{}", label(id)?);
                command("/bin/launchctl", &["disable", &service])?;
                if Command::new("/bin/launchctl")
                    .args(["print", &service])
                    .output()
                    .is_ok_and(|o| o.status.success())
                {
                    command("/bin/launchctl", &["bootout", &service])?;
                }
                Ok(())
            }
        }
    }
    pub fn remove(service: Service, id: &str) -> Result<(), String> {
        let target = match service {
            Service::Process => return Ok(()),
            Service::Systemd => dropin_path(id)?,
            Service::Launchd => plist_path(id)?,
        };
        if target.exists() {
            trusted(&target)?;
            std::fs::remove_file(&target).map_err(|e| e.to_string())?;
        }
        if service == Service::Systemd {
            let _ = std::fs::remove_dir(target.parent().expect("padre"));
            command(systemctl(), &["daemon-reload"])?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        const ID: &str = "00000000-0000-4000-8000-000012345678";
        #[test]
        fn names_reject_paths_and_noncanonical_ids() {
            assert_eq!(label(ID).unwrap(), "one.atenea.network.12345678");
            assert_eq!(unit(ID).unwrap(), format!("atenea-network@{ID}.service"));
            for id in ["../a", "--help", "00000000-0000-4000-8000-00001234567A", ""] {
                assert!(label(id).is_err());
            }
        }
        #[test]
        fn generated_plist_is_valid_xml_and_pins_its_configuration() {
            let text = plist(
                Path::new("/Library/Application Support/Atenea/redes & prueba"),
                ID,
            )
            .unwrap();
            let mut child = Command::new("python3").args(["-c", "import sys,plistlib; p=plistlib.loads(sys.stdin.buffer.read()); assert p['Label']=='one.atenea.network.12345678'; assert p['ProgramArguments'][1]=='00000000-0000-4000-8000-000012345678'; assert p['ProgramArguments'][2].endswith('redes & prueba'); assert list(p['KeepAlive']['PathState'].values())==[True]"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
            assert!(child.wait().unwrap().success());
        }
        #[test]
        fn dropin_escapes_systemd_specifiers_and_does_not_change_hardening() {
            let text = dropin(Path::new("/var/lib/redes % $ \"prueba\""), ID).unwrap();
            assert!(text.contains("redes %% $$ \\\"prueba\\\""));
            assert!(text.contains("RuntimeDirectory=atenea/12345678"));
            assert!(text.contains("ConditionPathExists=\n"));
            assert!(!text.contains("CapabilityBoundingSet"));
        }
        #[test]
        fn process_override_never_selects_a_service() {
            assert_eq!(
                choose(Backend::Process, Some(Service::Launchd)),
                Service::Process
            );
            assert_eq!(
                choose(Backend::Auto, Some(Service::Process)),
                Service::Process
            );
        }
    }
}

#[cfg(unix)]
pub use unix::*;
