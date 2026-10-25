//! Dispositivo L3: recibe y envía paquetes IP puros, sin cabecera de plataforma.

use std::io;

#[cfg(target_os = "linux")]
pub struct TunDevice {
    inner: atenea_tunnel::Tun,
}

// No puede existir un dispositivo sin un backend real.
#[cfg(not(target_os = "linux"))]
pub enum TunDevice {}

impl TunDevice {
    /// Abre el adaptador dentro del runtime Tokio.
    pub fn open(name: &str) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            atenea_tunnel::Tun::create(name).map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "TUN: backend de {} pendiente de implementación",
                    std::env::consts::OS
                ),
            ))
        }
    }

    pub fn name(&self) -> &str {
        #[cfg(target_os = "linux")]
        {
            self.inner.name()
        }
        #[cfg(not(target_os = "linux"))]
        match *self {}
    }

    /// Recibe un paquete IP completo; no incluye cabecera PI ni de familia.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            self.inner.read_packet(buf).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = buf;
            match *self {}
        }
    }

    /// Envía un paquete IP completo, sin añadir cabeceras de plataforma.
    pub async fn send(&self, packet: &[u8]) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            self.inner.write_packet(packet).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = packet;
            match *self {}
        }
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::TunDevice;
    use std::io;

    #[test]
    fn unsupported_platform_has_no_tun_device() {
        let err = TunDevice::open("atenea0").err().expect("sin backend TUN");
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert!(err.to_string().contains(std::env::consts::OS));
    }
}
