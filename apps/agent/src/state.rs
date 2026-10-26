// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Persistencia atómica del estado anti-rollback (contrato §3, "Estado persistente").

use crate::platform::private_files::PrivateFiles;
use atenea_identity::AgentState;
use std::io;
use std::path::{Path, PathBuf};

fn path(dir: &Path) -> PathBuf {
    dir.join("state.bin")
}

/// `Ok(None)` = primer arranque. Un fichero corrupto es un error: no se recrea en silencio.
pub fn load(dir: &Path) -> Result<Option<AgentState>, String> {
    match PrivateFiles::read(&path(dir)) {
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
    PrivateFiles::replace_state(dir, &state.to_bytes())
}

/// Estado anti-rollback en memoria y en disco. Es el único que lo escribe: el estado en memoria
/// solo cambia por `commit` (persistido a la vez) o por la hora observada, así que el disco nunca
/// retrocede respecto a lo ya comprometido.
pub struct Store {
    dir: PathBuf,
    state: AgentState,
    dirty: bool,
    last_saved: u64,
    /// Última instantánea comprometida, para verificar candidatas desde otras tareas.
    published: tokio::sync::watch::Sender<AgentState>,
}

impl Store {
    pub fn new(dir: PathBuf, state: AgentState, unix: u64) -> Store {
        Store {
            dir,
            published: tokio::sync::watch::channel(state.clone()).0,
            state,
            dirty: false,
            last_saved: unix,
        }
    }

    pub fn state(&self) -> &AgentState {
        &self.state
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<AgentState> {
        self.published.subscribe()
    }

    /// Persiste `state` y solo entonces lo adopta; antes de aplicar la configuración que lo originó.
    pub fn commit(&mut self, state: AgentState, unix: u64) -> io::Result<()> {
        save(&self.dir, &state)?;
        self.published.send_replace(state.clone());
        self.state = state;
        self.dirty = false;
        self.last_saved = unix;
        Ok(())
    }

    /// Avanza la hora máxima observada, sin pasar del fin del lease validado (`horizon`): un reloj
    /// adelantado no puede dejar el estado por delante de lo que firmó un emisor. Persiste como mucho cada 60 s.
    pub fn tick(&mut self, unix: u64, horizon: Option<u64>) {
        let seen = horizon.map_or(0, |h| unix.min(h));
        if seen > self.state.max_time_observed {
            self.state.max_time_observed = seen;
            self.dirty = true;
        }
        if self.dirty && unix.saturating_sub(self.last_saved) >= 60 {
            if let Err(e) = save(&self.dir, &self.state) {
                tracing::error!("no se pudo persistir el estado: {e}");
            } else {
                self.dirty = false;
            }
            self.last_saved = unix;
        }
    }

    pub fn flush(&self) {
        if self.dirty {
            let _ = save(&self.dir, &self.state);
        }
    }
}

/// Recuperación explícita de un reloj que estuvo adelantado: borra solo la hora máxima observada
/// y conserva versión, serial y anclas (el agente debe estar parado).
pub fn reset_clock(dir: &Path) -> Result<(), String> {
    let mut state = load(dir)?.ok_or("no hay estado persistido")?;
    state.max_time_observed = 0;
    save(dir, &state).map_err(|e| format!("{}: {e}", path(dir).display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

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

    fn sample() -> (AgentState, AgentState) {
        let old = AgentState::new([1; 16], [2; 16], [3; 32]);
        let mut new = old.clone();
        new.max_config_version = 5;
        new.max_issuer_serial = 3;
        new.anchors = vec![
            atenea_identity::signing_key([7; 32])
                .verifying_key()
                .to_bytes(),
        ];
        (old, new)
    }

    #[test]
    fn periodic_persistence_never_rolls_back_a_committed_state() {
        let d = tmpdir("c");
        let (old, new) = sample();
        let mut store = Store::new(d.clone(), old, 1000);
        store.commit(new.clone(), 1000).unwrap();
        store.tick(1200, Some(5000));
        let disk = load(&d).unwrap().unwrap();
        assert_eq!((disk.max_config_version, disk.max_issuer_serial), (5, 3));
        assert_eq!(disk.anchors, new.anchors);
        assert_eq!(disk.max_time_observed, 1200);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn observed_time_never_passes_the_validated_lease() {
        let d = tmpdir("d");
        let (old, _) = sample();
        let mut store = Store::new(d.clone(), old, 0);
        store.tick(2_000_000_000, Some(5000));
        assert_eq!(store.state().max_time_observed, 5000);
        store.tick(2_000_000_100, None);
        assert_eq!(store.state().max_time_observed, 5000);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_commit_keeps_the_previous_state() {
        let d = tmpdir("e");
        fs::create_dir_all(&d).unwrap();
        let blocked = d.join("f");
        fs::write(&blocked, b"x").unwrap();
        let (old, new) = sample();
        let mut store = Store::new(blocked, old.clone(), 0);
        assert!(store.commit(new, 1).is_err());
        assert_eq!(store.state(), &old);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn reset_clock_keeps_rollback_protection() {
        let d = tmpdir("g");
        let (_, mut new) = sample();
        new.max_time_observed = 2_000_000_000;
        save(&d, &new).unwrap();
        reset_clock(&d).unwrap();
        let after = load(&d).unwrap().unwrap();
        assert_eq!(after.max_time_observed, 0);
        assert_eq!(after.max_config_version, 5);
        assert_eq!(after.anchors, new.anchors);
        let _ = fs::remove_dir_all(&d);
    }
}
