//! Persistencia atómica del estado anti-rollback (contrato §3, "Estado persistente").

use atenea_identity::AgentState;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn path(dir: &Path) -> PathBuf {
    dir.join("state.bin")
}

/// `Ok(None)` = primer arranque. Un fichero corrupto es un error: no se recrea en silencio.
pub fn load(dir: &Path) -> Result<Option<AgentState>, String> {
    match fs::read(path(dir)) {
        Ok(b) => AgentState::from_bytes(&b).map(Some).map_err(|_| {
            format!(
                "{}: estado corrupto; reinicializar explícitamente",
                path(dir).display()
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path(dir).display())),
    }
}

/// Fichero temporal 0600 + fsync + rename + fsync del directorio.
pub fn save(dir: &Path, state: &AgentState) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("state.bin.tmp");
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(&state.to_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path(dir))?;
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atenea-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn roundtrip_missing_and_corrupt() {
        let d = tmpdir("a");
        assert!(load(&d).unwrap().is_none());
        let st = AgentState::new([1; 16], [2; 16], [3; 32]);
        save(&d, &st).unwrap();
        assert_eq!(load(&d).unwrap(), Some(st.clone()));
        // sobrescritura
        let mut st2 = st;
        st2.max_config_version = 7;
        save(&d, &st2).unwrap();
        assert_eq!(load(&d).unwrap(), Some(st2));
        // corrupción: error explícito, no primer arranque
        fs::write(path(&d), b"basura").unwrap();
        assert!(load(&d).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn save_failure_is_reported() {
        let d = tmpdir("b");
        fs::create_dir_all(&d).unwrap();
        // un fichero ocupa el lugar del directorio de estado
        let blocked = d.join("f");
        fs::write(&blocked, b"x").unwrap();
        assert!(save(&blocked, &AgentState::new([1; 16], [2; 16], [3; 32])).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
