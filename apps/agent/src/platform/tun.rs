//! Dispositivo L3: recibe y envía paquetes IP puros, sin cabecera de plataforma.

use std::io;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct TunDevice {
    inner: atenea_tunnel::Tun,
}

// No puede existir un dispositivo sin un backend real.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub enum TunDevice {}

impl TunDevice {
    /// Abre el adaptador dentro del runtime Tokio.
    pub fn open(name: &str) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            atenea_tunnel::Tun::create(name).map(|inner| Self { inner })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.name()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        match *self {}
    }

    /// Recibe un paquete IP completo; no incluye cabecera PI ni de familia.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.read_packet(buf).await
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = buf;
            match *self {}
        }
    }

    /// Envía un paquete IP completo, sin añadir cabeceras de plataforma.
    pub async fn send(&self, packet: &[u8]) -> io::Result<usize> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.write_packet(packet).await
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = packet;
            match *self {}
        }
    }
}

#[cfg(all(test, not(any(target_os = "linux", target_os = "macos"))))]
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
