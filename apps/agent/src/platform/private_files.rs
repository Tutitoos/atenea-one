// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Ficheros privados: permisos desde la creación y reemplazo duradero (ADR-007/008).

use std::fs::File;
use std::io;
use std::path::Path;
use zeroize::Zeroizing;

pub(crate) struct PrivateFiles;

#[cfg(unix)]
impl PrivateFiles {
    /// Abre un fichero regular privado: no es un enlace simbólico, sin acceso de grupo/otros y
    /// propiedad de root o del dueño de su directorio. Las comprobaciones son sobre el fd abierto.
    fn open_private(path: &Path) -> io::Result<File> {
        use std::os::unix::fs::MetadataExt;

        let deny = |what: &str| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                what.to_owned(),
            ))
        };
        let file = File::open(path)?;
        let meta = file.metadata()?;
        let link = std::fs::symlink_metadata(path)?;
        if link.file_type().is_symlink() || (link.dev(), link.ino()) != (meta.dev(), meta.ino()) {
            return deny("enlace simbólico o sustituido tras abrir");
        }
        if !meta.is_file() {
            return deny("no es un fichero regular");
        }
        if meta.mode() & 0o077 != 0 {
            return deny("permisos demasiado abiertos (se exige 0600)");
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if meta.uid() != 0 && meta.uid() != std::fs::metadata(parent)?.uid() {
            return deny("propietario inesperado");
        }
        Ok(file)
    }

    pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
        use std::io::Read;

        let mut bytes = Vec::new();
        Self::open_private(path)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn read_secret(path: &Path) -> Result<Zeroizing<String>, String> {
        use std::io::Read;

        let fail = |e: io::Error| format!("{}: {e}", path.display());
        let mut text = Zeroizing::new(String::new());
        Self::open_private(path)
            .and_then(|mut file| file.read_to_string(&mut text))
            .map_err(fail)?;
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

    /// Escribe `body` en un temporal exclusivo y lo promueve a `path` (fsync, rename, fsync del
    /// directorio); el temporal se retira si algo falla.
    fn replace_at(path: &Path, prefix: &str, body: &[u8]) -> io::Result<()> {
        use std::io::Write;

        let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
        let tmp = path.with_file_name(format!("{prefix}{suffix}.tmp"));
        let mut file = Self::create_new(&tmp)?;
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
        result
    }

    /// El directorio de estado se crea 0700 si no existe.
    pub(crate) fn replace_state(dir: &Path, body: &[u8]) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        Self::replace_at(&dir.join("state.bin"), ".state-", body)
    }

    /// La configuración usa un temporal exclusivo aleatorio y lo retira si falla.
    pub(crate) fn replace(path: &Path, body: &[u8]) -> Result<(), &'static str> {
        Self::replace_at(path, ".atenea-sync-", body).map_err(|_| "no se pudo guardar config_file")
    }
}

#[cfg(windows)]
impl PrivateFiles {
    pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
        atenea_windows::read_private(path)
    }
    pub(crate) fn read_secret(path: &Path) -> Result<Zeroizing<String>, String> {
        Self::read(path)
            .and_then(|bytes| String::from_utf8(bytes).map_err(io::Error::other))
            .map(Zeroizing::new)
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

    pub(crate) fn read_secret(_path: &Path) -> Result<Zeroizing<String>, String> {
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
        assert_eq!(PrivateFiles::read_secret(&path).unwrap().as_str(), "secret");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(PrivateFiles::read_secret(&path).is_err());
        PrivateFiles::replace(&path, b"replacement").unwrap();
        assert_eq!(
            PrivateFiles::read_secret(&path).unwrap().as_str(),
            "replacement"
        );
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

    fn scratch() -> std::path::PathBuf {
        let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
        let dir = std::env::temp_dir().join(format!("atenea-private-{suffix}"));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn private_reads_refuse_symbolic_links() {
        let dir = scratch();
        let real = dir.join("real");
        PrivateFiles::create_new(&real).unwrap();
        std::fs::write(&real, b"secret").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(PrivateFiles::read_secret(&link).is_err());
        assert!(PrivateFiles::read(&link).is_err());
        assert!(PrivateFiles::read(&real).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn state_replacement_never_follows_a_planted_temporary() {
        let dir = scratch();
        let victim = dir.join("victim");
        std::fs::write(&victim, b"intacto").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("state.bin.tmp")).unwrap();
        PrivateFiles::replace_state(&dir, b"state").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"intacto");
        assert_eq!(
            PrivateFiles::read(&dir.join("state.bin")).unwrap(),
            b"state"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_state_directory_is_private() {
        let dir = scratch();
        let state = dir.join("a").join("state");
        PrivateFiles::replace_state(&state, b"s").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&dir.join("a")), mode(&state)), (0o700, 0o700));
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
                assert_eq!(PrivateFiles::read_secret(&path).unwrap().as_str(), "secret");
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
