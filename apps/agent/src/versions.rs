// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Versiones instaladas de los componentes de Atenea (ADR-025): el agente en ejecución, la CLI y la aplicación de
//! escritorio. Es un dato de presentación: no autoriza ni enruta nada.
//!
//! La CLI y la aplicación se detectan ejecutando `--version` de una **ruta absoluta fija** por sistema, sin shell, con el
//! entorno vacío, plazo y tope de salida, y solo si la ruta es de confianza (root o Administradores, sin enlaces ni
//! escritura de otros). Cualquier fallo omite ese componente: no es un error del agente.
use serde::Serialize;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Uno por componente como máximo (ADR-025).
pub const MAX_VERSIONS: usize = 3;
const MAX_VERSION_CHARS: usize = 32;
/// Plazo de cada `--version` y tope de lo que se lee de su salida.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_OUTPUT_LIMIT: usize = 256;
/// Intentos de `exec` si el núcleo responde `ETXTBSY` (10 ms entre ellos: la ventana dura microsegundos).
const SPAWN_BUSY_RETRIES: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    Agent,
    Cli,
    Desktop,
}

impl Component {
    /// Nombre con el que cada binario imprime su línea de versión.
    fn binary(self) -> &'static str {
        match self {
            Self::Agent => "atenea-agent",
            Self::Cli => "atenea",
            Self::Desktop => "atenea-desktop",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "agent" => Some(Self::Agent),
            "cli" => Some(Self::Cli),
            "desktop" => Some(Self::Desktop),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComponentVersion {
    pub component: Component,
    pub version: String,
    pub commit: String,
}

/// Lo que el control plane devuelve de un par: la última lista informada y cuándo (RFC 3339, tal cual lo da el servidor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerVersions {
    pub list: Vec<ComponentVersion>,
    pub at: String,
}

/// De 1 a 32 caracteres de `0-9`, `A-Z`, `a-z`, `.`, `+` y `-` (las mismas reglas que valida el control plane).
pub fn valid_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_VERSION_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// De 7 a 40 hexadecimales en minúsculas, o `unknown`.
pub fn valid_commit(text: &str) -> bool {
    text == "unknown"
        || ((7..=40).contains(&text.len())
            && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Fecha de un informe tal cual la da el servidor: solo caracteres de una marca RFC 3339 y longitud acotada.
pub fn valid_reported_at(text: &str) -> bool {
    (20..=40).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'.' | b'+' | b'T' | b'Z'))
}

/// `<nombre> <versión> (<commit>)`, con un salto de línea final opcional y nada más.
pub fn parse_line(component: Component, output: &[u8]) -> Option<ComponentVersion> {
    let text = std::str::from_utf8(output).ok()?;
    let line = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let rest = line.strip_prefix(component.binary())?.strip_prefix(' ')?;
    let (version, tail) = rest.split_once(" (")?;
    let commit = tail.strip_suffix(')')?;
    (valid_version(version) && valid_commit(commit)).then(|| ComponentVersion {
        component,
        version: version.to_owned(),
        commit: commit.to_owned(),
    })
}

/// Lanza `programa --version` con el entorno vacío. Reintenta si el núcleo responde «fichero de texto ocupado»
/// (`ETXTBSY`): un `fork` concurrente de otro hilo puede heredar durante unos microsegundos el descriptor con el que
/// se acaba de escribir el binario (una actualización en curso, o un instalador) y `exec` falla aunque el fichero ya
/// esté cerrado. Pasa en cuanto ese hijo hace su propio `exec`.
fn spawn_version(program: &Path) -> Option<Child> {
    for attempt in 0..SPAWN_BUSY_RETRIES {
        match Command::new(program)
            .arg("--version")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => return Some(child),
            Err(e)
                if e.kind() == ErrorKind::ExecutableFileBusy
                    && attempt + 1 < SPAWN_BUSY_RETRIES =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return None,
        }
    }
    None
}

/// Ejecuta `programa --version` con el entorno vacío y devuelve su salida; `None` si no arranca, tarda más del plazo o
/// escribe más del tope.
fn run_version(program: &Path) -> Option<Vec<u8>> {
    let mut child = spawn_version(program)?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = (&mut stdout)
            .take((PROBE_OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut buffer);
        let _ = sender.send(buffer);
    });
    let started = Instant::now();
    let Ok(output) = receiver.recv_timeout(PROBE_TIMEOUT) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    // Cerró la salida: debe terminar enseguida; si no, se mata (un componente que se queda vivo no se informa).
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    (finished && output.len() <= PROBE_OUTPUT_LIMIT).then_some(output)
}

/// Decisión de confianza sobre un elemento de la ruta (Unix). Pura para poder probarla sin ser root.
#[cfg(unix)]
fn trusted_entry(is_symlink: bool, uid: u32, gid: u32, mode: u32, macos: bool) -> bool {
    // En macOS `/Applications` es de root con grupo `admin` (80) y escritura de grupo: los administradores ya son
    // privilegiados, así que ese grupo no cuenta como «otro usuario». Nada más con escritura de grupo u otros.
    let group_write_ok = macos && gid == 80;
    !is_symlink && uid == 0 && mode & 0o002 == 0 && (mode & 0o020 == 0 || group_write_ok)
}

/// La ruta es de confianza si el fichero es regular, ejecutable, de root y sin escritura de grupo ni otros, y cada
/// directorio hasta la raíz es de root, no es un enlace y no admite escritura de otros (ADR-025, decisión 2).
#[cfg(unix)]
fn is_trusted(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return false;
    }
    for (index, entry) in path.ancestors().enumerate() {
        let Ok(meta) = std::fs::symlink_metadata(entry) else {
            return false;
        };
        let macos = cfg!(target_os = "macos");
        if index == 0 {
            // El propio fichero: regular, ejecutable y sin escritura de grupo (ni en macOS).
            if !meta.file_type().is_file()
                || meta.mode() & 0o111 == 0
                || !trusted_entry(false, meta.uid(), meta.gid(), meta.mode(), false)
            {
                return false;
            }
        } else if !meta.file_type().is_dir()
            || !trusted_entry(false, meta.uid(), meta.gid(), meta.mode(), macos)
        {
            return false;
        }
    }
    true
}

#[cfg(windows)]
fn is_trusted(path: &Path) -> bool {
    atenea_windows::check_trusted_binary(path).is_ok()
}

/// Ruta absoluta fija de cada componente por sistema (ADR-025). `None` para el agente (se conoce a sí mismo).
#[cfg(target_os = "linux")]
fn fixed_path(component: Component) -> Option<PathBuf> {
    match component {
        Component::Agent => None,
        Component::Cli => Some("/usr/bin/atenea".into()),
        Component::Desktop => Some("/usr/bin/atenea-desktop".into()),
    }
}

#[cfg(target_os = "macos")]
fn fixed_path(component: Component) -> Option<PathBuf> {
    match component {
        Component::Agent => None,
        Component::Cli => Some("/Library/Atenea/bin/atenea".into()),
        Component::Desktop => {
            Some("/Applications/Atenea One.app/Contents/MacOS/atenea-desktop".into())
        }
    }
}

/// El servicio corre como SYSTEM: su entorno lo fija el sistema, no un usuario sin privilegios.
#[cfg(windows)]
fn fixed_path(component: Component) -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("ProgramFiles")?).join("Atenea");
    match component {
        Component::Agent => None,
        Component::Cli => Some(base.join("atenea.exe")),
        Component::Desktop => Some(base.join("atenea-desktop.exe")),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn fixed_path(_: Component) -> Option<PathBuf> {
    None
}

/// Versión y commit del agente en ejecución (los de su compilación).
pub fn running_agent(version: &str, commit: &str) -> Option<ComponentVersion> {
    (valid_version(version) && valid_commit(commit)).then(|| ComponentVersion {
        component: Component::Agent,
        version: version.to_owned(),
        commit: commit.to_owned(),
    })
}

/// Marca que la aplicación lleva dentro cuando entiende `--version`. Una aplicación anterior a esa opción la ignoraría y
/// **arrancaría entera** (bandeja y ventana) al ejecutarla: por eso el agente busca la marca en el binario antes de
/// ejecutarlo, y sin ella no lo ejecuta ni lo informa.
pub const DESKTOP_MARKER: &[u8] = b"ATENEA-DESKTOP-VERSION-FLAG-1";
/// Tope de lo que se lee del binario buscando la marca.
const MARKER_SCAN_LIMIT: u64 = 128 * 1024 * 1024;

/// ¿Contiene el fichero la marca? Lectura en bloques con solape para no perderla en el corte.
fn contains_marker(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = file.take(MARKER_SCAN_LIMIT);
    let mut window: Vec<u8> = Vec::with_capacity(64 * 1024 + DESKTOP_MARKER.len());
    let mut block = vec![0u8; 64 * 1024];
    loop {
        let Ok(read) = reader.read(&mut block) else {
            return false;
        };
        if read == 0 {
            return false;
        }
        window.extend_from_slice(&block[..read]);
        if window
            .windows(DESKTOP_MARKER.len())
            .any(|chunk| chunk == DESKTOP_MARKER)
        {
            return true;
        }
        let keep = DESKTOP_MARKER.len() - 1;
        if window.len() > keep {
            window.drain(..window.len() - keep);
        }
    }
}

/// Detecta lo instalado a partir de las rutas dadas. Un componente cuya ruta no es de confianza, una aplicación sin la
/// marca de `--version` o una salida que no cumple la forma estricta se omiten.
fn detect(
    paths: &[(Component, PathBuf)],
    trusted: impl Fn(&Path) -> bool,
) -> Vec<ComponentVersion> {
    paths
        .iter()
        .filter_map(|(component, path)| {
            if !trusted(path) {
                return None;
            }
            if *component == Component::Desktop && !contains_marker(path) {
                return None;
            }
            parse_line(*component, &run_version(path)?)
        })
        .collect()
}

/// La lista que informa este equipo: el agente en ejecución y, de la CLI y la aplicación, lo que haya instalado.
pub fn installed(agent: Option<ComponentVersion>) -> Vec<ComponentVersion> {
    let paths: Vec<(Component, PathBuf)> = [Component::Cli, Component::Desktop]
        .into_iter()
        .filter_map(|c| Some((c, fixed_path(c)?)))
        .collect();
    let mut list: Vec<ComponentVersion> = agent.into_iter().collect();
    list.extend(detect(&paths, is_trusted));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "dfe72aefc123edc288d1daead1c812825e0a2a00";

    #[test]
    fn the_version_line_has_one_strict_shape() {
        let ok = |component, text: &str| parse_line(component, text.as_bytes());
        let cli = ok(Component::Cli, &format!("atenea 0.1.31 ({COMMIT})\n")).unwrap();
        assert_eq!(
            (cli.component, cli.version.as_str(), cli.commit.as_str()),
            (Component::Cli, "0.1.31", COMMIT)
        );
        assert!(ok(Component::Desktop, "atenea-desktop 0.0.0-dev (unknown)").is_some());
        assert!(
            ok(
                Component::Desktop,
                "atenea-desktop 1.2.3-rc.1+b2 (abcdef0)\r\n"
            )
            .is_some()
        );
        for bad in [
            "",
            "\n",
            "atenea",
            "atenea 0.1.31",
            "atenea 0.1.31 ()",
            "atenea  0.1.31 (abcdef0)",
            "atenea 0.1.31 (abcdef0) extra",
            "atenea 0.1.31 (abcdef0)\nsegunda línea",
            "atenea 0.1.31 (ABCDEF0)",
            "atenea 0.1.31 (abcdef)",
            "atenea 0.1.31 (Unknown)",
            "atenea 0.1 31 (abcdef0)",
            "atenea 0.1.31; id (abcdef0)",
            "atenea-agent 0.1.31 (abcdef0)",
            "atenea-desktop 0.1.31 (abcdef0)",
        ] {
            assert!(ok(Component::Cli, bad).is_none(), "{bad:?}");
        }
        assert!(parse_line(Component::Cli, &[0xff, 0xfe]).is_none());
        assert!(
            ok(
                Component::Cli,
                &format!("atenea {} (abcdef0)", "1".repeat(33))
            )
            .is_none()
        );
        assert!(
            ok(
                Component::Cli,
                &format!("atenea {} (abcdef0)", "1".repeat(32))
            )
            .is_some()
        );
    }

    #[test]
    fn components_round_trip_the_names_the_control_plane_uses() {
        for (text, component) in [
            ("agent", Component::Agent),
            ("cli", Component::Cli),
            ("desktop", Component::Desktop),
        ] {
            assert_eq!(Component::parse(text), Some(component));
            assert_eq!(serde_json::to_value(component).unwrap(), text);
        }
        for bad in ["", "Agent", "relay", "agent "] {
            assert!(Component::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_reported_date_accepts_only_what_a_timestamp_has() {
        assert!(valid_reported_at("2026-11-16T10:20:30Z"));
        assert!(valid_reported_at("2026-11-16T10:20:30.123456+00:00"));
        for bad in [
            "",
            "ayer",
            "2026-11-16 10:20:30",
            &"1".repeat(41),
            "2026-11-16T10:20:30Z<b>",
        ] {
            assert!(!valid_reported_at(bad), "{bad:?}");
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};

        struct Dir(PathBuf);
        impl Dir {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir()
                    .join(format!("atenea-versions-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
            fn script(&self, name: &str, body: &str) -> PathBuf {
                let path = self.0.join(name);
                std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                path
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn found(path: &Path, component: Component) -> Vec<ComponentVersion> {
            detect(&[(component, path.to_owned())], |_| true)
        }

        #[test]
        fn a_well_behaved_binary_is_detected_and_the_rest_are_left_out() {
            let dir = Dir::new("detect");
            let cli = dir.script("atenea", &format!("echo 'atenea 0.1.31 ({COMMIT})'"));
            let found_cli = found(&cli, Component::Cli);
            assert_eq!(found_cli.len(), 1);
            assert_eq!(found_cli[0].version, "0.1.31");
            // Otro nombre, otra forma, sin línea, código de salida distinto de cero y un binario que no existe.
            assert!(
                found(
                    &dir.script("otro", "echo 'atenea-desktop 0.1.31 (abcdef0)'"),
                    Component::Cli
                )
                .is_empty()
            );
            assert!(found(&dir.script("basura", "echo hola"), Component::Cli).is_empty());
            assert!(found(&dir.script("vacio", "true"), Component::Cli).is_empty());
            assert!(
                found(
                    &dir.script("falla", "echo 'atenea 1.0.0 (abcdef0)'; exit 3"),
                    Component::Cli
                )
                .is_empty()
            );
            assert!(found(&dir.0.join("no-existe"), Component::Cli).is_empty());
        }

        #[test]
        fn a_path_that_is_not_trusted_is_never_executed() {
            let dir = Dir::new("trust");
            let marker = dir.0.join("ejecutado");
            let script = dir.script(
                "atenea",
                &format!(
                    "touch '{}'; echo 'atenea 0.1.31 (abcdef0)'",
                    marker.display()
                ),
            );
            assert!(detect(&[(Component::Cli, script)], |_| false).is_empty());
            assert!(
                !marker.exists(),
                "se ejecutó un binario que no es de confianza"
            );
        }

        #[test]
        fn output_over_the_limit_and_a_binary_that_hangs_are_left_out_in_time() {
            let dir = Dir::new("limits");
            let long = dir.script(
                "larga",
                "echo 'atenea 0.1.31 (abcdef0)'; head -c 400 /dev/zero | tr '\\0' x",
            );
            assert!(found(&long, Component::Cli).is_empty());
            let hang = dir.script("cuelga", "exec /bin/sleep 30");
            let started = Instant::now();
            assert!(found(&hang, Component::Cli).is_empty());
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "{:?}",
                started.elapsed()
            );
            // Cierra la salida pero no termina.
            let alive = dir.script(
                "vivo",
                "echo 'atenea 0.1.31 (abcdef0)'; exec >&-; exec /bin/sleep 30",
            );
            let started = Instant::now();
            assert!(found(&alive, Component::Cli).is_empty());
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "{:?}",
                started.elapsed()
            );
        }

        #[test]
        fn the_environment_is_empty() {
            let dir = Dir::new("env");
            // Con el entorno heredado de la prueba habría decenas de variables; con `env_clear` solo quedan las que
            // añade el propio intérprete de órdenes.
            let script = dir.script(
                "atenea",
                "[ \"$(/usr/bin/env | /usr/bin/wc -l)\" -le 5 ] && echo 'atenea 0.1.31 (abcdef0)'",
            );
            assert!(
                std::env::vars_os().count() > 5,
                "la prueba solo distingue si el proceso hereda más de cinco variables"
            );
            assert_eq!(found(&script, Component::Cli).len(), 1);
        }

        #[test]
        fn an_app_without_the_version_flag_marker_is_never_executed() {
            let dir = Dir::new("marker");
            let ran = dir.0.join("ejecutado");
            // Una aplicación anterior a `--version`: ignoraría la opción y arrancaría entera.
            let old = dir.script(
                "atenea-desktop",
                &format!(
                    "touch '{}'; echo 'atenea-desktop 0.1.31 (abcdef0)'",
                    ran.display()
                ),
            );
            assert!(found(&old, Component::Desktop).is_empty());
            assert!(!ran.exists(), "se ejecutó una aplicación sin la marca");
            // Con la marca dentro del binario, sí se ejecuta y se informa.
            let new = dir.script(
                "atenea-desktop",
                &format!(
                    "# {}\necho 'atenea-desktop 0.1.32 (abcdef0)'",
                    String::from_utf8_lossy(DESKTOP_MARKER)
                ),
            );
            let list = found(&new, Component::Desktop);
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].version, "0.1.32");
            // La marca solo se exige a la aplicación: la CLI nunca la necesita.
            assert_eq!(
                found(
                    &dir.script("atenea", "echo 'atenea 0.1.31 (abcdef0)'"),
                    Component::Cli
                )
                .len(),
                1
            );
        }

        #[test]
        fn the_marker_is_found_across_read_blocks_and_only_when_present() {
            let dir = Dir::new("scan");
            let path = dir.0.join("binario");
            for offset in [
                0usize,
                1,
                65_536 - 10,
                65_536 - 1,
                65_536,
                65_536 + 7,
                200_000,
            ] {
                let mut bytes = vec![b'x'; offset];
                bytes.extend_from_slice(DESKTOP_MARKER);
                bytes.extend_from_slice(&[b'y'; 50]);
                std::fs::write(&path, &bytes).unwrap();
                assert!(
                    contains_marker(&path),
                    "marca en el desplazamiento {offset}"
                );
            }
            // Una marca cortada o con otra versión de la marca no cuenta.
            let mut cut = vec![b'x'; 70_000];
            cut.extend_from_slice(&DESKTOP_MARKER[..DESKTOP_MARKER.len() - 1]);
            std::fs::write(&path, &cut).unwrap();
            assert!(!contains_marker(&path));
            std::fs::write(&path, b"ATENEA-DESKTOP-VERSION-FLAG-2").unwrap();
            assert!(!contains_marker(&path));
            assert!(!contains_marker(&dir.0.join("no-existe")));
        }

        #[test]
        fn trust_rules_reject_links_other_owners_and_writable_paths() {
            // (¿enlace?, uid, gid, modo, macOS) -> ¿de confianza?
            assert!(trusted_entry(false, 0, 0, 0o755, false));
            assert!(trusted_entry(false, 0, 0, 0o750, false));
            assert!(!trusted_entry(true, 0, 0, 0o755, false), "enlace");
            assert!(
                !trusted_entry(false, 501, 20, 0o755, false),
                "no es de root"
            );
            assert!(
                !trusted_entry(false, 0, 0, 0o757, false),
                "escritura de otros"
            );
            assert!(
                !trusted_entry(false, 0, 0, 0o775, false),
                "escritura de grupo"
            );
            assert!(
                !trusted_entry(false, 0, 20, 0o775, true),
                "grupo escribible que no es admin"
            );
            assert!(
                trusted_entry(false, 0, 80, 0o775, true),
                "/Applications: grupo admin en macOS"
            );
            assert!(
                !trusted_entry(false, 0, 80, 0o775, false),
                "el grupo 80 solo vale en macOS"
            );
            assert!(
                !trusted_entry(false, 0, 80, 0o777, true),
                "escritura de otros aunque sea admin"
            );
        }

        #[test]
        fn the_real_check_rejects_symlinks_and_files_the_current_user_owns() {
            let dir = Dir::new("real");
            let script = dir.script("atenea", "echo 'atenea 0.1.31 (abcdef0)'");
            // Un fichero de un usuario sin privilegios nunca es de confianza (salvo que la prueba corra como root).
            let root = std::fs::metadata(&script)
                .map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
                .unwrap();
            if !root {
                assert!(!is_trusted(&script));
            }
            let link = dir.0.join("enlace");
            symlink(&script, &link).unwrap();
            assert!(!is_trusted(&link));
            assert!(!is_trusted(Path::new("atenea")), "ruta relativa");
            assert!(!is_trusted(&dir.0.join("../atenea")), "con ..");
            assert!(!is_trusted(&dir.0.join("no-existe")));
        }
    }

    #[test]
    fn the_running_agent_is_reported_only_with_a_valid_identity() {
        assert!(running_agent("0.1.31", COMMIT).is_some());
        assert!(running_agent("0.1.31", "unknown").is_some());
        assert!(running_agent("", COMMIT).is_none());
        assert!(running_agent("0.1.31", "XYZ").is_none());
    }
}
