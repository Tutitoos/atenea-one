//! Ficheros privados: permisos desde la creación y reemplazo duradero (ADR-007/008).

use std::fs::File;
use std::io;
use std::path::Path;

pub(crate) struct PrivateFiles;

#[cfg(unix)]
impl PrivateFiles {
    pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub(crate) fn read_secret(path: &Path) -> Result<String, String> {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;

        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let meta = file
            .metadata()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "{}: permisos demasiado abiertos (se exige 0600)",
                path.display()
            ));
        }
        let mut text = String::new();
        (&file)
            .read_to_string(&mut text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(text)
    }

    pub(crate) fn create_new(path: &Path) -> io::Result<File> {
        use std::os::unix::fs::OpenOptionsExt;

        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }

    /// El estado conserva su temporal fijo truncado y no lo elimina si falla.
    pub(crate) fn replace_state(dir: &Path, body: &[u8]) -> io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("state.bin.tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        std::fs::rename(&tmp, dir.join("state.bin"))?;
        File::open(dir)?.sync_all()
    }

    /// La configuración usa un temporal exclusivo aleatorio y lo retira si falla.
    pub(crate) fn replace(path: &Path, body: &[u8]) -> Result<(), &'static str> {
        use std::io::Write;

        let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
        let tmp = path.with_file_name(format!(".atenea-sync-{suffix}.tmp"));
        let mut file = Self::create_new(&tmp).map_err(|_| "no se pudo crear el temporal")?;
        let result = (|| {
            file.write_all(body)?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)?;
            File::open(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?
            .sync_all()
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|_| "no se pudo guardar config_file")
    }
}

#[cfg(windows)]
impl PrivateFiles {
    pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
        atenea_windows::read_private(path)
    }
    pub(crate) fn read_secret(path: &Path) -> Result<String, String> {
        Self::read(path)
            .and_then(|bytes| String::from_utf8(bytes).map_err(io::Error::other))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
    pub(crate) fn create_new(path: &Path) -> io::Result<File> {
        atenea_windows::create_private(path)
    }
    pub(crate) fn replace_state(dir: &Path, body: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        atenea_windows::replace_private(&dir.join("state.bin"), body)
    }
    pub(crate) fn replace(path: &Path, body: &[u8]) -> Result<(), &'static str> {
        atenea_windows::replace_private(path, body).map_err(|_| "no se pudo guardar config_file")
    }
}

#[cfg(not(any(unix, windows)))]
impl PrivateFiles {
    const UNSUPPORTED: &'static str =
        "ficheros privados no soportados: backend con DACL restrictiva pendiente";

    pub(crate) fn read(_path: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            Self::UNSUPPORTED,
        ))
    }

    pub(crate) fn read_secret(_path: &Path) -> Result<String, String> {
        Err(Self::UNSUPPORTED.into())
    }

    pub(crate) fn create_new(_path: &Path) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            Self::UNSUPPORTED,
        ))
    }

    pub(crate) fn replace_state(_dir: &Path, _body: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            Self::UNSUPPORTED,
        ))
    }

    pub(crate) fn replace(_path: &Path, _body: &[u8]) -> Result<(), &'static str> {
        Err(Self::UNSUPPORTED)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_creation_never_overwrites_and_replacements_are_private() {
        let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
        let dir = std::env::temp_dir().join(format!("atenea-private-{suffix}"));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("secret");
        PrivateFiles::create_new(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&path, b"secret").unwrap();
        assert_eq!(
            PrivateFiles::create_new(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(PrivateFiles::read_secret(&path).unwrap(), "secret");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(PrivateFiles::read_secret(&path).is_err());
        PrivateFiles::replace(&path, b"replacement").unwrap();
        assert_eq!(PrivateFiles::read_secret(&path).unwrap(), "replacement");
        PrivateFiles::replace_state(&dir, b"state").unwrap();
        assert_eq!(
            PrivateFiles::read(&dir.join("state.bin")).unwrap(),
            b"state"
        );
        assert_eq!(
            std::fs::metadata(dir.join("state.bin"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn private_wrappers_replace_or_reject_unelevated_access() {
        let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
        let dir = std::env::temp_dir().join(format!("atenea-private-{suffix}"));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("secret");
        match PrivateFiles::create_new(&path) {
            Ok(file) => {
                drop(file);
                assert_eq!(
                    PrivateFiles::create_new(&path).unwrap_err().kind(),
                    io::ErrorKind::AlreadyExists
                );
                PrivateFiles::replace(&path, b"secret").unwrap();
                assert_eq!(PrivateFiles::read_secret(&path).unwrap(), "secret");
                PrivateFiles::replace_state(&dir, b"state").unwrap();
                assert_eq!(
                    PrivateFiles::read(&dir.join("state.bin")).unwrap(),
                    b"state"
                );
            }
            Err(error) => {
                assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
                assert!(!path.exists());
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(test, not(any(unix, windows))))]
mod tests {
    use super::*;

    #[test]
    fn refuses_private_io_without_a_dacl_backend() {
        let path = Path::new("private-files-must-not-be-created");
        assert_eq!(
            PrivateFiles::read(path).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(
            PrivateFiles::read_secret(path)
                .unwrap_err()
                .contains("DACL")
        );
        assert_eq!(
            PrivateFiles::create_new(path).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            PrivateFiles::replace_state(path, b"secret")
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert!(
            PrivateFiles::replace(path, b"secret")
                .unwrap_err()
                .contains("DACL")
        );
    }
}
