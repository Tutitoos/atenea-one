//! Control local autenticado y socket separado de solo lectura.
use crate::platform::LocalIpc;
use crate::platform::local_ipc::{Listener, Stream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Stop,
    Reload,
    Cleanup,
}

pub fn bind(path: &Path, status_gid: Option<u32>, read_only: bool) -> Result<Listener, String> {
    LocalIpc::bind(path, status_gid, read_only)
}

const CONNECTION_LIMIT: usize = 16;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

/// Cada listener conserva su propia cota; la lectura de estado no ocupa el canal de control.
pub struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn serve(
    listener: Listener,
    socket: PathBuf,
    read_only: bool,
    status: watch::Receiver<Arc<str>>,
    actions: mpsc::Sender<Action>,
) -> Server {
    Server(tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = clients.join_next(), if !clients.is_empty() => {}
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) if clients.len() < CONNECTION_LIMIT => {
                        let socket = socket.clone();
                        let status = status.clone();
                        let actions = actions.clone();
                        clients.spawn(async move {
                            let _ = tokio::time::timeout(
                                CONNECTION_TIMEOUT,
                                handle(stream, &socket, read_only, status, &actions),
                            ).await;
                        });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!("aceptación IPC: {error}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }))
}
#[cfg(unix)]
fn allowed(uid: u32, owner: u32) -> bool {
    uid == 0 || uid == owner
}
fn action(command: &str, read_only: bool) -> Result<Action, &'static str> {
    match command {
        "status" => Ok(Action::None),
        _ if read_only => Err("socket de estado: solo se permite status"),
        "stop" => Ok(Action::Stop),
        "reload" => Ok(Action::Reload),
        "cleanup" => Ok(Action::Cleanup),
        _ => Err("orden desconocida"),
    }
}
async fn handle(
    stream: Stream,
    _socket: &Path,
    read_only: bool,
    status: watch::Receiver<Arc<str>>,
    actions: &mpsc::Sender<Action>,
) {
    #[cfg(unix)]
    if !read_only {
        #[cfg(unix)]
        let authorized = LocalIpc::peer_uid(&stream)
            .ok()
            .zip(LocalIpc::socket_owner(_socket).ok())
            .is_some_and(|(uid, owner)| allowed(uid, owner));
        if !authorized {
            return;
        }
    }
    let mut reader = BufReader::new(stream);
    // Límite por byte: ningún cliente local puede hacer crecer la línea sin cota.
    let mut line = Vec::new();
    let read = async {
        loop {
            let bytes = reader.fill_buf().await?;
            if bytes.is_empty() {
                return Ok::<(), std::io::Error>(());
            }
            let byte = bytes[0];
            reader.consume(1);
            if byte == b'\n' {
                return Ok(());
            }
            line.push(byte);
            if line.len() > 32 {
                return Err(std::io::Error::other("orden demasiado larga"));
            }
        }
    }
    .await;
    if read.is_err() {
        return;
    }
    // Windows vincula la impersonación al último mensaje leído del cliente.
    #[cfg(windows)]
    if !read_only && !LocalIpc::authorized(reader.get_ref()) {
        return;
    }
    let command = String::from_utf8_lossy(&line);
    let (reply, pending): (Arc<str>, _) = match action(command.trim(), read_only) {
        Ok(Action::None) => (status.borrow().clone(), None),
        Ok(result) => match actions.try_reserve() {
            Ok(permit) => (Arc::from("{\"ok\":true}"), Some((permit, result))),
            Err(_) => (Arc::from("{\"error\":\"control ocupado\"}"), None),
        },
        Err(error) => (
            Arc::from(serde_json::json!({"error":error}).to_string()),
            None,
        ),
    };
    let _ = reader.get_mut().write_all(reply.as_bytes()).await;
    let _ = reader.get_mut().write_all(b"\n").await;
    if let Some((permit, action)) = pending {
        permit.send(action);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_uid_cannot_control_and_status_cannot_stop() {
        #[cfg(unix)]
        {
            assert!(allowed(0, 100));
            assert!(allowed(100, 100));
            assert!(!allowed(101, 100));
        }
        assert!(action("stop", true).is_err());
        assert!(action("cleanup", true).is_err());
        assert!(action("reload", true).is_err());
        assert_eq!(action("status", true), Ok(Action::None));
        assert_eq!(action("stop", false), Ok(Action::Stop));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn silent_clients_do_not_stall_packets_timers_or_control() {
        use std::os::unix::fs::DirBuilderExt;
        use tokio::io::AsyncReadExt;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            PathBuf::from("/tmp").join(format!("atenea-serve-{}-{unique}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let path = dir.join("status");
        let control = dir.join("control");
        let (snapshots, status) = watch::channel(Arc::<str>::from("{\"ready\":true}"));
        let (actions, mut commands) = mpsc::channel(16);
        let server = serve(
            bind(&path, None, true).unwrap(),
            path.clone(),
            true,
            status.clone(),
            actions.clone(),
        );
        let controller = serve(
            bind(&control, None, false).unwrap(),
            control.clone(),
            false,
            status,
            actions.clone(),
        );
        snapshots.send_replace(Arc::from("{\"ready\":false}"));
        let mut query = tokio::net::UnixStream::connect(&path).await.unwrap();
        query.write_all(b"status\n").await.unwrap();
        let mut reply = String::new();
        query.read_to_string(&mut reply).await.unwrap();
        assert_eq!(reply, "{\"ready\":false}\n");
        let mut query = tokio::net::UnixStream::connect(&path).await.unwrap();
        query.write_all(b"reload\n").await.unwrap();
        let mut reply = String::new();
        query.read_to_string(&mut reply).await.unwrap();
        assert!(reply.contains("solo se permite status"));
        assert!(commands.try_recv().is_err());
        let mut silent = Vec::new();
        for _ in 0..CONNECTION_LIMIT {
            silent.push(tokio::net::UnixStream::connect(&path).await.unwrap());
            tokio::task::yield_now().await;
        }
        let (mut tun, mut producer) = tokio::net::UnixStream::pair().unwrap();
        producer.write_all(b"packet").await.unwrap();
        let mut timer = tokio::time::interval(Duration::from_millis(10));
        let mut client = tokio::net::UnixStream::connect(&control).await.unwrap();
        client.write_all(b"reload\n").await.unwrap();
        let mut packets = 0;
        let mut ticks = 0;
        let mut reloaded = false;
        tokio::time::timeout(Duration::from_millis(250), async {
            let mut buf = [0; 6];
            while packets == 0 || ticks < 3 || !reloaded {
                tokio::select! {
                    count = tun.read(&mut buf), if packets == 0 => {
                        assert_eq!(count.unwrap(), 6);
                        assert_eq!(&buf, b"packet");
                        packets += 1;
                    }
                    _ = timer.tick() => ticks += 1,
                    action = commands.recv(), if !reloaded => {
                        assert_eq!(action, Some(Action::Reload));
                        reloaded = true;
                    }
                }
            }
        })
        .await
        .expect("el plano de datos progresa antes del plazo IPC de un segundo");
        let mut reply = String::new();
        client.read_to_string(&mut reply).await.unwrap();
        assert_eq!(reply, "{\"ok\":true}\n");
        // El plazo también libera las conexiones que no envían una orden.
        tokio::time::timeout(
            Duration::from_secs(2),
            silent[0].read_to_string(&mut String::new()),
        )
        .await
        .unwrap()
        .unwrap();
        for _ in 0..16 {
            actions.try_send(Action::Stop).unwrap();
        }
        let mut busy = tokio::net::UnixStream::connect(&control).await.unwrap();
        busy.write_all(b"reload\n").await.unwrap();
        let mut reply = String::new();
        busy.read_to_string(&mut reply).await.unwrap();
        assert_eq!(reply, "{\"error\":\"control ocupado\"}\n");
        assert_eq!(commands.len(), 16);
        drop((server, controller, silent));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
