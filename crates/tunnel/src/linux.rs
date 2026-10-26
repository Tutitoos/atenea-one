// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Interfaz TUN L3 (IP sin cabecera PI) para Linux.
//!
//! Al cerrar el descriptor (drop) el kernel elimina la interfaz, con sus direcciones y rutas:
//! no queda configuración residual aunque el proceso muera.

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use tokio::io::unix::AsyncFd;

const TUNSETIFF: libc::c_ulong = 0x4004_54ca;
const IFF_TUN: libc::c_short = 0x0001;
const IFF_NO_PI: libc::c_short = 0x1000;
const IFNAMSIZ: usize = 16;

#[repr(C)]
struct IfReq {
    name: [u8; IFNAMSIZ],
    flags: libc::c_short,
    _pad: [u8; 22],
}

pub struct Tun {
    fd: AsyncFd<OwnedFd>,
    name: String,
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() < IFNAMSIZ
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Tun {
    /// Crea la interfaz (requiere CAP_NET_ADMIN). Debe llamarse dentro de un runtime Tokio.
    pub fn create(name: &str) -> io::Result<Tun> {
        if !valid_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "nombre de interfaz inválido",
            ));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/net/tun")?;
        let mut req = IfReq {
            name: [0; IFNAMSIZ],
            flags: IFF_TUN | IFF_NO_PI,
            _pad: [0; 22],
        };
        req.name[..name.len()].copy_from_slice(name.as_bytes());
        #[allow(unsafe_code)]
        // SAFETY: `req` es un `ifreq` válido y vive durante la llamada; el fd es de /dev/net/tun.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), TUNSETIFF, &mut req as *mut IfReq) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        let end = req.name.iter().position(|&b| b == 0).unwrap_or(IFNAMSIZ);
        let name = String::from_utf8_lossy(&req.name[..end]).into_owned();
        #[allow(unsafe_code)]
        // SAFETY: `OwnedFd` posee un descriptor válido, abierto con O_NONBLOCK, y AsyncFd lo conserva
        // sin reemplazarlo ni cerrarlo por otra vía.
        let fd = unsafe { AsyncFd::register(OwnedFd::from(file)) }.map_err(io::Error::from)?;
        Ok(Tun { fd, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Lee un paquete IP completo.
    pub async fn read_packet(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            #[allow(unsafe_code)]
            // SAFETY: `buf` es un búfer válido de `buf.len()` bytes y el fd está abierto.
            let r = guard.try_io(|inner| {
                let n =
                    unsafe { libc::read(inner.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            if let Ok(res) = r {
                return res;
            }
        }
    }

    /// Escribe un paquete IP completo.
    pub async fn write_packet(&self, pkt: &[u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.writable().await?;
            #[allow(unsafe_code)]
            // SAFETY: `pkt` es un búfer válido de `pkt.len()` bytes y el fd está abierto.
            let r = guard.try_io(|inner| {
                let n = unsafe { libc::write(inner.as_raw_fd(), pkt.as_ptr().cast(), pkt.len()) };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            if let Ok(res) = r {
                return res;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn interface_names() {
        assert!(valid_name("atenea0"));
        assert!(!valid_name(""));
        assert!(!valid_name("0123456789abcdef")); // 16 bytes
        assert!(!valid_name("a b"));
        assert!(!valid_name("a;rm"));
    }
}
