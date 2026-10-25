//! Utilidades Unix de pruebas: publicación de scripts y procesos frente a forks paralelos.
use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

pub fn fake_binary(path: &Path, script: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(script.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
    File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

fn retry_text_busy<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match operation() {
            // Otro test puede heredar el fd de escritura entre fork y exec, aun con CLOEXEC.
            Err(error) if error.raw_os_error() == Some(26) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

pub fn output(command: &mut Command) -> io::Result<Output> {
    retry_text_busy(|| command.output())
}

pub fn spawn(command: &mut Command) -> io::Result<Child> {
    retry_text_busy(|| command.spawn())
}

#[test]
fn retries_only_text_busy_errors() {
    let mut attempts = 0;
    let value = retry_text_busy(|| {
        attempts += 1;
        if attempts == 1 {
            Err(io::Error::from_raw_os_error(26))
        } else {
            Ok(7)
        }
    })
    .unwrap();
    assert_eq!((value, attempts), (7, 2));
    let mut attempts = 0;
    let error = retry_text_busy::<()>(|| {
        attempts += 1;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    })
    .unwrap_err();
    assert_eq!(attempts, 1);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}
