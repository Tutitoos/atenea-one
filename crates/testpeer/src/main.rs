mod peer;

use atenea_crypto::{StaticKeypair, hex};
use atenea_identity::DeviceBundle;
use clap::Parser;
use peer::{Addresses, Peer};
use std::io::Write;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// Par ICMP de laboratorio, sin TUN ni privilegios.
#[derive(Parser)]
struct Args {
    #[arg(long)]
    device_dir: PathBuf,
    #[arg(long)]
    agent_bundle: PathBuf,
    #[arg(long, default_value = "127.0.0.1:51821")]
    listen: SocketAddr,
    #[arg(long)]
    own_addr: Ipv4Addr,
    #[arg(long)]
    own_addr6: Option<Ipv6Addr>,
    #[arg(long)]
    agent_addr: Ipv4Addr,
    #[arg(long)]
    agent_addr6: Option<Ipv6Addr>,
    #[arg(long, default_value = "00112233445566778899aabbccddeeff")]
    network_id: String,
    #[arg(long, default_value_t = 1)]
    count: u64,
    #[arg(long, default_value_t = 60)]
    timeout_secs: u64,
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if args.own_addr6.is_some() != args.agent_addr6.is_some() {
        return Err("--own-addr6 y --agent-addr6 deben usarse juntos".into());
    }
    let network =
        hex::decode(&args.network_id).ok_or("--network-id debe tener 32 dígitos hexadecimales")?;
    let path = args.device_dir.join("noise.key");
    let file = std::fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err("noise.key debe ser privada (sin permisos de grupo/otros)".into());
        }
    }
    use std::io::Read;
    let mut key = Zeroizing::new(String::new());
    file.take(4096).read_to_string(&mut key)?;
    let private = Zeroizing::new(
        hex::decode::<32>(&key).ok_or("noise.key inválida (se esperan 32 bytes hexadecimales)")?,
    );
    let bundle = DeviceBundle::parse(&std::fs::read(args.agent_bundle)?)?;
    bundle.verify()?;
    let socket = UdpSocket::bind(args.listen)?;
    let start = Instant::now();
    let timeout = Duration::from_secs(args.timeout_secs);
    let mut peer = Peer::new(
        StaticKeypair::from_private(&private),
        bundle.noise_pubkey,
        network,
        Addresses {
            own: args.own_addr,
            agent: args.agent_addr,
            own6: args.own_addr6,
            agent6: args.agent_addr6,
        },
        start,
    );
    println!("ready");
    std::io::stdout().flush()?;
    // Un byte extra permite rechazar datagramas truncados que superen el máximo del protocolo.
    let mut buffer = [0u8; atenea_protocol::MAX_DATAGRAM + 1];
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        while peer.echo_replies < args.count {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err("timeout sin alcanzar --count".into());
            }
            socket.set_read_timeout(Some(remaining.min(Duration::from_secs(1))))?;
            match socket.recv_from(&mut buffer) {
                Ok((len, from)) => {
                    let now = Instant::now();
                    let unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                    if let Some(reply) = peer.receive(from, &buffer[..len], now, unix) {
                        socket.send_to(&reply, from)?;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
            if let Some((to, keepalive)) = peer.keepalive(Instant::now()) {
                socket.send_to(&keepalive, to)?;
            }
        }
        Ok(())
    })();
    println!("echo_replies={}", peer.echo_replies);
    eprintln!("discarded={}", peer.discarded);
    result
}

fn main() -> std::process::ExitCode {
    match run(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("atenea-testpeer: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
