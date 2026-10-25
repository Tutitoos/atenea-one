use atenea_identity::SigningKey;
use atenea_relay::{Relay, config::Config};
use atenea_relay_core::{self as w, Message, RelayToken};
use rustls::pki_types::{CertificateDer, pem::PemObject};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::watch,
    task::JoinHandle,
    time::timeout,
};
use tokio_rustls::{TlsConnector, client::TlsStream};

fn sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn sk(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}
struct Lab {
    dir: PathBuf,
    udp: SocketAddr,
    tcp: SocketAddr,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<std::io::Result<()>>>,
    connector: TlsConnector,
    client: Arc<rustls::ClientConfig>,
}
impl Lab {
    async fn start(edit: impl FnOnce(&mut Config)) -> Self {
        let dir = std::env::temp_dir().join(format!("atenea-relay-test-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        std::fs::write(dir.join("cert.pem"), cert.cert.pem()).unwrap();
        std::fs::write(dir.join("key.pem"), cert.signing_key.serialize_pem()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.cert.der().clone()).unwrap();
        let mut client =
            rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
                .with_root_certificates(roots)
                .with_no_client_auth();
        client.alpn_protocols = vec![w::ALPN.to_vec()];
        let mut c = Config {
            udp_listen: "127.0.0.1:0".parse().unwrap(),
            tcp_listen: "127.0.0.1:0".parse().unwrap(),
            metrics_listen: "127.0.0.1:0".into(),
            coordinator_keys: vec![hex::encode(sk(1).verifying_key().to_bytes())],
            tls_cert_file: dir.join("cert.pem").display().to_string(),
            tls_key_file: dir.join("key.pem").display().to_string(),
            status_socket: dir.join("status.sock").display().to_string(),
            ..Config::default()
        };
        edit(&mut c);
        let relay = Relay::bind(c).await.unwrap();
        let udp = relay.udp_addr().unwrap();
        let tcp = relay.tcp_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let task = tokio::spawn(relay.run(rx));
        let client = Arc::new(client);
        Self {
            dir,
            udp,
            tcp,
            stop,
            task: Some(task),
            connector: TlsConnector::from(client.clone()),
            client,
        }
    }
    async fn udp(&self) -> Client {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket.connect(self.udp).await.unwrap();
        Client::Udp(socket)
    }
    async fn tls(&self) -> Client {
        let tcp = TcpStream::connect(self.tcp).await.unwrap();
        let tls = self
            .connector
            .connect("localhost".try_into().unwrap(), tcp)
            .await
            .unwrap();
        Client::Tls(Box::new(tls))
    }
    async fn close(mut self) {
        self.stop.send(true).unwrap();
        self.task.take().unwrap().await.unwrap().unwrap();
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
enum Client {
    Udp(UdpSocket),
    Tls(Box<TlsStream<TcpStream>>),
}
impl Client {
    async fn send(&mut self, body: &[u8]) {
        match self {
            Self::Udp(s) => {
                s.send(body).await.unwrap();
            }
            Self::Tls(s) => s.write_all(&w::encode_frame(body).unwrap()).await.unwrap(),
        }
    }
    async fn raw_recv(&mut self) -> Vec<u8> {
        match self {
            Self::Udp(s) => {
                let mut b = [0; 2048];
                let n = s.recv(&mut b).await.unwrap();
                b[..n].to_vec()
            }
            Self::Tls(s) => {
                let n = s.read_u16().await.unwrap();
                let mut b = vec![0; usize::from(n)];
                s.read_exact(&mut b).await.unwrap();
                b
            }
        }
    }
    async fn recv(&mut self) -> Vec<u8> {
        loop {
            let b = timeout(Duration::from_secs(2), self.raw_recv())
                .await
                .unwrap();
            if !matches!(w::parse(&b), Ok(Message::PeerState { .. })) {
                return b;
            }
        }
    }
    async fn silent(&mut self) {
        assert!(
            timeout(Duration::from_millis(80), self.recv())
                .await
                .is_err()
        );
    }
    async fn cookie(&mut self) -> [u8; 16] {
        match self {
            Self::Tls(_) => [0; 16],
            Self::Udp(_) => {
                self.send(&w::encode_cookie_req()).await;
                let b = self.recv().await;
                assert!(b.len() < w::COOKIE_REQ_LEN);
                match w::parse(&b).unwrap() {
                    Message::Cookie { cookie } => cookie,
                    _ => panic!(),
                }
            }
        }
    }
    async fn alloc(&mut self, src: u8, dst: u8) -> u64 {
        let cookie = self.cookie().await;
        let t = token(src, dst, [7; 16], sec(), &sk(1));
        self.send(&request(&cookie, &t, &sk(src), sec())).await;
        match w::parse(&self.recv().await).unwrap() {
            Message::AllocOk { alloc_id, .. } => alloc_id,
            m => panic!("{m:?}"),
        }
    }
}
fn token(src: u8, dst: u8, net: [u8; 16], now: u64, issuer: &SigningKey) -> Vec<u8> {
    RelayToken::issue(
        issuer,
        net,
        [src; 16],
        [dst; 16],
        sk(src).verifying_key().to_bytes(),
        now,
        now + 300,
        [9; 8],
    )
    .encode()
    .to_vec()
}
fn request(cookie: &[u8; 16], token: &[u8], key: &SigningKey, ts: u64) -> Vec<u8> {
    w::encode_alloc(cookie, ts, token, &w::sign_alloc(key, cookie, ts, token))
}
async fn transfer(a: &mut Client, aid: u64, b: &mut Client, bid: u64) {
    a.send(&w::encode_data(aid, b"\x01\x02ciphertext")).await;
    assert_eq!(
        w::parse(&b.recv().await).unwrap(),
        Message::Data {
            alloc_id: bid,
            datagram: b"\x01\x02ciphertext"
        }
    );
}

#[tokio::test]
async fn udp_tls_and_mixed_bidirectional() {
    let lab = Lab::start(|_| {}).await;
    for mode in 0..3 {
        let mut a = if mode == 1 {
            lab.tls().await
        } else {
            lab.udp().await
        };
        let mut b = if mode == 0 {
            lab.udp().await
        } else {
            lab.tls().await
        };
        let aid = a.alloc(2, 3).await;
        let bid = b.alloc(3, 2).await;
        transfer(&mut a, aid, &mut b, bid).await;
        transfer(&mut b, bid, &mut a, aid).await;
    }
    let mut status = tokio::net::UnixStream::connect(lab.dir.join("status.sock"))
        .await
        .unwrap();
    let mut b = vec![];
    status.read_to_end(&mut b).await.unwrap();
    let s: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(s["traffic"]["forwarded_packets"], 6);
    lab.close().await;
}
#[tokio::test]
async fn unpaired_unbound_and_rebinding() {
    let lab = Lab::start(|_| {}).await;
    let mut a = lab.udp().await;
    let aid = a.alloc(2, 3).await;
    a.send(&w::encode_data(aid, b"xx")).await;
    a.silent().await;
    let mut b = lab.udp().await;
    let bid = b.alloc(3, 2).await;
    let mut attacker = lab.udp().await;
    attacker.send(&w::encode_data(aid, b"xx")).await;
    b.silent().await;
    assert_eq!(attacker.alloc(2, 3).await, aid);
    a.send(&w::encode_data(aid, b"xx")).await;
    b.silent().await;
    transfer(&mut attacker, aid, &mut b, bid).await;
    lab.close().await;
}
#[tokio::test]
async fn invalid_tokens_proofs_cookies_and_time() {
    let lab = Lab::start(|c| c.alloc_per_second = 100).await;
    let mut a = lab.udp().await;
    let cookie = a.cookie().await;
    let n = sec();
    let good = token(2, 3, [7; 16], n, &sk(1));
    for (t, key, ts, code) in [
        (
            token(2, 3, [7; 16], n, &sk(9)),
            sk(2),
            n,
            w::ErrCode::BadToken,
        ),
        (
            token(2, 3, [7; 16], n - 1000, &sk(1)),
            sk(2),
            n,
            w::ErrCode::Expired,
        ),
        (good.clone(), sk(4), n, w::ErrCode::BadSignature),
        (good.clone(), sk(2), n - 31, w::ErrCode::Expired),
        (vec![0; w::TOKEN_LEN], sk(2), n, w::ErrCode::BadToken),
    ] {
        a.send(&request(&cookie, &t, &key, ts)).await;
        assert_eq!(w::parse(&a.recv().await), Ok(Message::AllocErr { code }));
    }
    for offset in [5, 21, 37, 172] {
        let mut changed = good.clone();
        changed[offset] ^= 1;
        a.send(&request(&cookie, &changed, &sk(2), n)).await;
        assert_eq!(
            w::parse(&a.recv().await),
            Ok(Message::AllocErr {
                code: w::ErrCode::BadToken
            })
        );
    }
    let mut other = lab.udp().await;
    other.send(&request(&cookie, &good, &sk(2), n)).await;
    other.silent().await;
    a.send(&request(&[0; 16], &good, &sk(2), n)).await;
    a.silent().await;
    // Otra red o dispositivo no forma pareja aunque su token sea auténtico.
    let aid = a.alloc(2, 3).await;
    let c = other.cookie().await;
    let wrong = token(3, 2, [8; 16], n, &sk(1));
    other.send(&request(&c, &wrong, &sk(3), n)).await;
    let _ = other.recv().await;
    a.send(&w::encode_data(aid, b"xx")).await;
    other.silent().await;
    let wrong_device = token(3, 4, [7; 16], n, &sk(1));
    other.send(&request(&c, &wrong_device, &sk(3), n)).await;
    let _ = other.recv().await;
    a.send(&w::encode_data(aid, b"xx")).await;
    other.silent().await;
    lab.close().await;
}
#[tokio::test]
async fn capacity_rates_size_and_tcp_frames() {
    let lab = Lab::start(|c| {
        c.max_allocations = 2;
        c.max_per_network = 2;
        c.max_per_ip = 2;
        c.cookie_per_second = 1;
        c.alloc_per_second = 100;
        c.max_datagram = 4;
    })
    .await;
    let mut a = lab.tls().await;
    let aid = a.alloc(2, 3).await;
    let mut b = lab.tls().await;
    let _ = b.alloc(3, 2).await;
    let mut c = lab.tls().await;
    let cookie = [0; 16];
    let n = sec();
    c.send(&request(
        &cookie,
        &token(4, 5, [7; 16], n, &sk(1)),
        &sk(4),
        n,
    ))
    .await;
    assert_eq!(
        w::parse(&c.recv().await),
        Ok(Message::AllocErr {
            code: w::ErrCode::Full
        })
    );
    a.send(&w::encode_data(aid, b"oversize")).await;
    b.silent().await;
    let mut udp = lab.udp().await;
    let _ = udp.cookie().await;
    udp.send(&w::encode_cookie_req()).await;
    udp.silent().await;
    for length in [0u16, 2049] {
        let mut tls = lab.tls().await;
        if let Client::Tls(s) = &mut tls {
            s.write_all(&length.to_be_bytes()).await.unwrap();
            let mut byte = [0];
            assert!(
                timeout(Duration::from_secs(2), s.read(&mut byte))
                    .await
                    .unwrap()
                    .map_or(true, |n| n == 0)
            );
        }
    }
    lab.close().await;
}
#[tokio::test]
async fn tcp_frame_before_alloc_closes_connection() {
    let lab = Lab::start(|_| {}).await;
    let mut tls = lab.tls().await;
    tls.send(&w::encode_data(1, b"xx")).await;
    if let Client::Tls(s) = &mut tls {
        let mut byte = [0];
        assert!(
            timeout(Duration::from_secs(2), s.read(&mut byte))
                .await
                .unwrap()
                .map_or(true, |n| n == 0)
        );
    }
    lab.close().await;
}
#[tokio::test]
async fn partial_frames_and_alloc_idle_handshake_deadlines() {
    let lab = Lab::start(|c| {
        c.alloc_secs = 1;
        c.idle_secs = 1;
        c.handshake_secs = 1;
    })
    .await;
    let mut a = lab.tls().await;
    let n = sec();
    let frame = w::encode_frame(&request(
        &[0; 16],
        &token(2, 3, [7; 16], n, &sk(1)),
        &sk(2),
        n,
    ))
    .unwrap();
    if let Client::Tls(s) = &mut a {
        for chunk in frame.chunks(3) {
            s.write_all(chunk).await.unwrap();
        }
    }
    assert!(matches!(
        w::parse(&a.recv().await),
        Ok(Message::AllocOk { .. })
    ));
    let mut idle = lab.tls().await;
    let mut raw = TcpStream::connect(lab.tcp).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1150)).await;
    for client in [&mut a, &mut idle] {
        if let Client::Tls(s) = client {
            let mut b = [0; 2048];
            loop {
                match timeout(Duration::from_secs(2), s.read(&mut b))
                    .await
                    .unwrap()
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        }
    }
    let mut b = [0];
    assert_eq!(raw.read(&mut b).await.unwrap(), 0);
    lab.close().await;
}
#[tokio::test]
async fn proxy_v2_real_ip_and_alpn_rejection() {
    let lab = Lab::start(|c| {
        c.proxy_protocol = true;
        c.tcp_per_ip = 1;
    })
    .await;
    let mut tcp = TcpStream::connect(lab.tcp).await.unwrap();
    let mut h = b"\r\n\r\n\0\r\nQUIT\n\x21\x11\x00\x0c".to_vec();
    h.extend_from_slice(&[192, 0, 2, 1, 127, 0, 0, 1, 0, 1, 0, 2]);
    tcp.write_all(&h).await.unwrap();
    let tls = lab
        .connector
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    let mut a = Client::Tls(Box::new(tls));
    let _ = a.alloc(2, 3).await;
    // El límite ya corresponde a 192.0.2.1, no al loopback del balanceador.
    let mut tcp = TcpStream::connect(lab.tcp).await.unwrap();
    tcp.write_all(&h).await.unwrap();
    assert!(
        lab.connector
            .connect("localhost".try_into().unwrap(), tcp)
            .await
            .is_err()
    );
    lab.close().await;
    let lab = Lab::start(|_| {}).await;
    let mut client = (*lab.client).clone();
    client.alpn_protocols = vec![b"wrong".to_vec()];
    let connector = TlsConnector::from(Arc::new(client));
    let tcp = TcpStream::connect(lab.tcp).await.unwrap();
    match connector
        .connect("localhost".try_into().unwrap(), tcp)
        .await
    {
        Err(_) => {}
        Ok(mut s) => {
            let mut b = [0];
            assert!(s.read(&mut b).await.map_or(true, |n| n == 0));
        }
    }
    lab.close().await;
}

#[tokio::test]
async fn tls12_and_connection_limits() {
    let lab = Lab::start(|_| {}).await;
    let mut roots = rustls::RootCertStore::empty();
    let pem = std::fs::read(lab.dir.join("cert.pem")).unwrap();
    for cert in CertificateDer::pem_slice_iter(&pem) {
        roots.add(cert.unwrap()).unwrap();
    }
    let mut c = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS12])
        .with_root_certificates(roots)
        .with_no_client_auth();
    c.alpn_protocols = vec![w::ALPN.to_vec()];
    let connector = TlsConnector::from(Arc::new(c));
    assert!(
        connector
            .connect(
                "localhost".try_into().unwrap(),
                TcpStream::connect(lab.tcp).await.unwrap()
            )
            .await
            .is_err()
    );
    lab.close().await;
    for total in [false, true] {
        let lab = Lab::start(|c| {
            if total {
                c.tcp_total = 1;
                c.tcp_per_ip = 1;
            } else {
                c.tcp_per_ip = 1;
            }
        })
        .await;
        let _first = lab.tls().await;
        let mut second = TcpStream::connect(lab.tcp).await.unwrap();
        let mut byte = [0];
        assert_eq!(
            timeout(Duration::from_secs(2), second.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        lab.close().await;
    }
}
