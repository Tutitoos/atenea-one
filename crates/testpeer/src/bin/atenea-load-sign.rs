//! Fixtures y firmas del laboratorio de carga; nunca usar estas claves fuera del lab.
use atenea_identity::{self as id, DeviceConfig, DnsConfig, Peer, Prefix};
use ed25519_dalek::Signer;
use std::{
    env, fs,
    io::{self, BufRead, Write},
    net::IpAddr,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn bytes(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2) && s.is_ascii(), "hex inválido");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex inválido"))
        .collect()
}
fn fixed<const N: usize>(s: &str) -> [u8; N] {
    bytes(s).try_into().expect("longitud hex inválida")
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("reloj")
        .as_secs()
}
fn main() {
    let a: Vec<String> = env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("raw-sign") => {
            let stdin = io::stdin();
            let mut out = io::BufWriter::new(io::stdout().lock());
            for line in stdin.lock().lines() {
                let line = line.expect("stdin");
                let (seed, message) = line.split_once('\t').expect("seedhex TAB messagehex");
                let signature = id::signing_key(fixed(seed)).sign(&bytes(message));
                writeln!(out, "{}", hex(&signature.to_bytes())).expect("stdout");
                out.flush().expect("flush");
            }
        }
        Some("wire-cookie") => {
            assert_eq!(a.len(), 5);
            let public = fixed::<32>(&a[2]);
            let init = bytes(&a[3]);
            let reply = bytes(&a[4]);
            assert!(
                atenea_crypto::cookie::verify_mac1(&init, &public),
                "INIT/MAC1 inválido"
            );
            let atenea_protocol::Packet::Init { sender_index, .. } =
                atenea_protocol::parse(&init).expect("INIT")
            else {
                panic!("INIT requerido")
            };
            let cookie = atenea_crypto::cookie::open_reply(
                &reply,
                sender_index,
                &public,
                init[114..130].try_into().expect("MAC1"),
            )
            .expect("COOKIE_REPLY");
            println!("{}", hex(cookie.as_ref()));
        }
        Some("sign") => {
            // Una línea seed device method path ts nonce bodyhex; respuesta cabecera.
            let stdin = io::stdin();
            let mut out = io::BufWriter::new(io::stdout().lock());
            for line in stdin.lock().lines() {
                let line = line.expect("stdin");
                let p: Vec<_> = line.split('\t').collect();
                assert_eq!(p.len(), 7);
                let key = id::signing_key(fixed(p[0]));
                writeln!(
                    out,
                    "{}",
                    id::api_auth::sign_request(
                        &key,
                        fixed(p[1]),
                        p[2],
                        p[3],
                        &bytes(p[6]),
                        p[4].parse().expect("timestamp"),
                        fixed(p[5])
                    )
                )
                .expect("stdout");
                out.flush().expect("flush");
            }
        }
        Some("fixtures") => {
            assert_eq!(a.len(), 5, "fixtures COUNT PER_NETWORK DIR");
            let count: usize = a[2].parse().expect("COUNT");
            let group: usize = a[3].parse().expect("PER_NETWORK");
            assert!(count > 0 && (2..=256).contains(&group));
            let dir = Path::new(&a[4]);
            fs::create_dir(dir).expect("DIR debe ser nuevo");
            let root_seed = match env::var("ATENEA_LOAD_ROOT_KEY") {
                Ok(path) => fixed(fs::read_to_string(path).expect("root key").trim()),
                Err(_) => id::generate_seed(),
            };
            let root = id::signing_key(root_seed);
            let issuer_seed = match env::var("ATENEA_LOAD_ISSUER_KEY") {
                Ok(path) => fixed(fs::read_to_string(path).expect("issuer key").trim()),
                Err(_) => id::generate_seed(),
            };
            let issuer = id::signing_key(issuer_seed);
            fs::write(dir.join("root.pub"), hex(&root.verifying_key().to_bytes())).expect("root");
            fs::write(dir.join("issuer.key"), hex(&issuer_seed)).expect("issuer");
            let t = now();
            let mut manifest =
                io::BufWriter::new(fs::File::create(dir.join("devices.tsv")).expect("manifest"));
            for start in (0..count).step_by(group) {
                let end = (start + group).min(count);
                assert!(
                    end - start >= 2,
                    "el último grupo necesita al menos dos dispositivos"
                );
                let network = id::generate_seed()[..16].try_into().expect("network");
                let cert = id::IssuerCert::issue(
                    &root,
                    network,
                    1,
                    t - 1,
                    t + 86400,
                    issuer.verifying_key().to_bytes(),
                    None,
                )
                .expect("cert");
                fs::write(dir.join(format!("{}.cert", hex(&network))), cert.encode())
                    .expect("cert file");
                let seeds: Vec<_> = (start..end).map(|_| id::generate_seed()).collect();
                let ids: Vec<[u8; 16]> = (start..end)
                    .map(|_| id::generate_seed()[..16].try_into().expect("device"))
                    .collect();
                let noises: Vec<_> = (start..end).map(|_| id::generate_seed()).collect();
                for j in 0..end - start {
                    let key = id::signing_key(seeds[j]);
                    let peer = (j + 1) % (end - start);
                    // Pares en estrella no: todos los vecinos listan a todos (máximo256).
                    let peers = (0..end - start)
                        .filter(|k| *k != j)
                        .map(|k| Peer {
                            device_id: ids[k],
                            noise_pubkey: noises[k],
                            addrs: vec![IpAddr::from([
                                10,
                                90,
                                ((k + 1) / 256) as u8,
                                ((k + 1) % 256) as u8,
                            ])],
                            endpoints: vec![],
                            routes: vec![],
                        })
                        .collect();
                    let mut cfg = DeviceConfig {
                        network_id: network,
                        device_id: ids[j],
                        issuer_serial: 1,
                        config_version: t,
                        not_before: t - 1,
                        not_after: t + 3599,
                        noise_pubkey: noises[j],
                        identity_pubkey: key.verifying_key().to_bytes(),
                        mtu: 1400,
                        tunnel_addrs: vec![IpAddr::from([
                            10,
                            90,
                            ((j + 1) / 256) as u8,
                            ((j + 1) % 256) as u8,
                        ])],
                        network_prefixes: vec![Prefix {
                            addr: IpAddr::from([10, 90, 0, 0]),
                            len: 16,
                        }],
                        peers,
                        rules: vec![],
                        advertised: vec![],
                        dns: DnsConfig::default(),
                    };
                    cfg.canonicalize();
                    let signed = id::issue_config(&issuer, &cert, &cfg).expect("config");
                    id::inspect_config(&signed, &[root.verifying_key().to_bytes()], t)
                        .expect("fixture verificable");
                    fs::write(dir.join(format!("{}.config", hex(&ids[j]))), signed)
                        .expect("config file");
                    let bundle = id::DeviceBundle::create(ids[j], noises[j], &key);
                    bundle.verify().expect("bundle verificable");
                    writeln!(
                        manifest,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        hex(&network),
                        hex(&ids[j]),
                        hex(&seeds[j]),
                        hex(&bundle.encode()),
                        hex(&noises[j]),
                        hex(&key.verifying_key().to_bytes()),
                        hex(&ids[peer]),
                        j + 1
                    )
                    .expect("manifest row");
                }
            }
        }
        _ => panic!("fixtures COUNT PER_NETWORK DIR | sign | wire-cookie PUB INIT REPLY"),
    }
}
