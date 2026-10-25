use crate::{Runtime, alloc::Binding, config::Config, limits::ip_group, now};
use atenea_relay_core::{ALPN, FrameDecoder, MAGIC, Message, encode_frame, parse, ty};
use std::{
    fs::File,
    io::{self, BufReader},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_rustls::TlsAcceptor;

pub fn tls_config(c: &Config) -> io::Result<TlsAcceptor> {
    let certs = rustls_pemfile::certs(&mut BufReader::new(File::open(&c.tls_cert_file)?))
        .collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(File::open(&c.tls_key_file)?))?
        .ok_or_else(|| io::Error::other("falta clave PEM"))?;
    let mut config =
        rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(io::Error::other)?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.max_early_data_size = 0;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
async fn proxy(stream: &mut TcpStream, source: SocketAddr) -> io::Result<IpAddr> {
    if !source.ip().is_loopback() {
        return Err(io::Error::other("PROXY no loopback"));
    }
    let mut h = [0; 16];
    stream.read_exact(&mut h).await?;
    if &h[..12] != b"\r\n\r\n\0\r\nQUIT\n" || h[12] != 0x21 {
        return Err(io::Error::other("cabecera PROXY inválida"));
    }
    let len = usize::from(u16::from_be_bytes([h[14], h[15]]));
    // Solo TCP4/TCP6, sin TLV: cabecera acotada y sin ambigüedad de origen.
    match (h[13], len) {
        (0x11, 12) => {
            let mut b = [0; 12];
            stream.read_exact(&mut b).await?;
            Ok(IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])))
        }
        (0x21, 36) => {
            let mut b = [0; 36];
            stream.read_exact(&mut b).await?;
            Ok(IpAddr::V6(Ipv6Addr::from(
                <[u8; 16]>::try_from(&b[..16]).unwrap(),
            )))
        }
        _ => Err(io::Error::other("familia o longitud PROXY inválida")),
    }
}
pub async fn connection(
    runtime: Arc<Runtime>,
    stream: TcpStream,
    source: SocketAddr,
    id: u64,
    rx: mpsc::Receiver<Vec<u8>>,
    tls: TlsAcceptor,
    mut shutdown: watch::Receiver<bool>,
) {
    let result = tokio::select! {
        _ = shutdown.changed() => Ok(()),
        r = serve(&runtime,stream,source,id,rx,tls) => r,
    };
    let outputs = {
        let mut s = runtime.shared.lock().unwrap();
        s.clients.remove(&id);
        if result.is_err() {
            s.state.drop_packet("tcp_protocol");
        }
        let ids = s
            .state
            .table
            .entries
            .values()
            .filter(|a| matches!(a.binding,Binding::Tcp(n,_) if n == id))
            .map(|a| a.id)
            .collect();
        s.state.remove(ids, now())
    };
    runtime.dispatch(outputs).await;
}
async fn serve(
    runtime: &Runtime,
    mut stream: TcpStream,
    source: SocketAddr,
    id: u64,
    mut rx: mpsc::Receiver<Vec<u8>>,
    tls: TlsAcceptor,
) -> io::Result<()> {
    let c = runtime.shared.lock().unwrap().state.config.clone();
    let handshake = async {
        let ip = ip_group(if c.proxy_protocol {
            proxy(&mut stream, source).await?
        } else {
            source.ip()
        });
        {
            let mut s = runtime.shared.lock().unwrap();
            if s.clients
                .iter()
                .filter(|(n, (a, _))| **n != id && *a == ip)
                .count()
                >= c.tcp_per_ip
            {
                return Err(io::Error::other("límite TCP por IP"));
            }
            if let Some((a, _)) = s.clients.get_mut(&id) {
                *a = ip;
            }
        }
        let stream = tls.accept(stream).await?;
        if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
            return Err(io::Error::other("ALPN obligatorio"));
        }
        Ok((stream, ip))
    };
    let (stream, ip) = timeout(Duration::from_secs(c.handshake_secs), handshake)
        .await
        .map_err(io::Error::other)??;
    let binding = Binding::Tcp(id, ip);
    let (mut read, mut write) = tokio::io::split(stream);
    let mut decoder = FrameDecoder::new();
    let mut buf = [0; 2048];
    let alloc_deadline = Instant::now() + Duration::from_secs(c.alloc_secs);
    let mut allocated = false;
    let mut last_rx = Instant::now();
    loop {
        let deadline = if allocated {
            last_rx + Duration::from_secs(c.idle_secs)
        } else {
            alloc_deadline
        };
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err(io::Error::other("plazo TCP agotado")),
            body = rx.recv() => {
                let Some(body) = body else { return Ok(()); };
                let frame = encode_frame(&body).map_err(|_| io::Error::other("trama inválida"))?;
                // Un receptor que no lee no puede retener la conexión indefinidamente.
                tokio::time::timeout_at(deadline,write.write_all(&frame)).await.map_err(io::Error::other)??;
            },
            n = read.read(&mut buf) => {
                let n = n?; if n == 0 { return Ok(()); } last_rx = Instant::now(); decoder.push(&buf[..n]);
                while let Some(body) = decoder.next_frame().map_err(|_| io::Error::other("longitud de trama inválida"))? {
                    if matches!(parse(&body),Ok(Message::CookieReq | Message::Cookie {..})) { return Err(io::Error::other("cookie prohibida en TCP")); }
                    // Antes de asignarse solo cabe ALLOC: el resto no puede ser válido y costaría mutex global.
                    if !allocated && !body.starts_with(&[MAGIC, ty::ALLOC]) { return Err(io::Error::other("trama previa a ALLOC")); }
                    allocated |= runtime.message(binding,&body).await;
                }
            }
        }
    }
}
