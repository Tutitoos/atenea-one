//! Sonda de permisos para CI: nunca interpreta ENOENT ni un fallo de ejecución como rechazo válido.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn classify(result: std::io::Result<()>, deny: bool) -> Result<&'static str, String> {
    match (result, deny) {
        (Ok(()), false) => Ok("connected"),
        (Err(error), true) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Ok("permission_denied")
        }
        (Ok(()), true) => Err("el usuario ajeno pudo conectar al socket".into()),
        (Err(error), _) => Err(format!("conexión no verificada: {error}")),
    }
}

#[cfg(unix)]
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || !matches!(args[1].to_str(), Some("allow" | "deny")) {
        return Err("uso: atenea-ipc-probe SOCKET allow|deny".into());
    }
    let connected = std::os::unix::net::UnixStream::connect(&args[0]).map(|_| ());
    println!("{}", classify(connected, args[1] == "deny")?);
    Ok(())
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    let result = run();
    #[cfg(not(unix))]
    let result: Result<(), String> = Err("la sonda requiere sockets Unix".into());
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("atenea-ipc-probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::classify;
    use std::io::{Error, ErrorKind};

    #[test]
    fn only_eacces_counts_as_permission_enforcement() {
        assert_eq!(
            classify(Err(Error::from(ErrorKind::PermissionDenied)), true).unwrap(),
            "permission_denied"
        );
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::ConnectionRefused,
            ErrorKind::TimedOut,
        ] {
            assert!(classify(Err(Error::from(kind)), true).is_err());
        }
        assert!(classify(Ok(()), true).is_err());
        assert_eq!(classify(Ok(()), false).unwrap(), "connected");
        assert!(classify(Err(Error::from(ErrorKind::PermissionDenied)), false).is_err());
    }

    #[test]
    fn existing_socket_connects_and_missing_socket_is_not_a_valid_denial() {
        use std::os::unix::net::{UnixListener, UnixStream};
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("atenea-probe-{}-{unique}", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        assert_eq!(
            classify(UnixStream::connect(&path).map(|_| ()), false).unwrap(),
            "connected"
        );
        assert!(classify(UnixStream::connect(&path).map(|_| ()), true).is_err());
        drop(listener);
        std::fs::remove_file(&path).unwrap();
        assert!(classify(UnixStream::connect(&path).map(|_| ()), true).is_err());
    }
}
