// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! utun entrega una cabecera de familia en orden de red; el agente solo ve IP.
#![allow(unsafe_code)]
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use tokio::io::unix::AsyncFd;

const CTLIOCGINFO: libc::c_ulong = 0xc064_4e03;
#[repr(C)]
struct ControlInfo {
    id: u32,
    name: [u8; 96],
}
#[repr(C)]
struct ControlAddr {
    len: u8,
    family: u8,
    sysaddr: u16,
    id: u32,
    unit: u32,
    reserved: [u32; 5],
}

pub struct Tun {
    fd: AsyncFd<OwnedFd>,
    name: String,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "paquete utun/IP inválido")
}
fn family(packet: &[u8]) -> io::Result<u32> {
    match packet.first().map(|v| v >> 4) {
        Some(4)
            if packet.len() >= 20
                && usize::from(packet[0] & 15) * 4 >= 20
                && usize::from(packet[0] & 15) * 4 <= packet.len()
                && usize::from(u16::from_be_bytes([packet[2], packet[3]])) == packet.len() =>
        {
            Ok(2)
        }
        Some(6)
            if packet.len() >= 40
                && usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 == packet.len() =>
        {
            Ok(30)
        }
        _ => Err(invalid()),
    }
}
fn normalize(header: [u8; 4], packet: &[u8]) -> io::Result<usize> {
    if u32::from_be_bytes(header) != family(packet)? {
        return Err(invalid());
    }
    Ok(packet.len())
}
impl Tun {
    /// `utun` deja elegir al kernel; `utunN` solicita el índice N (sc_unit=N+1).
    pub fn create(name: &str) -> io::Result<Self> {
        let unit = if name == "utun" {
            0
        } else {
            name.strip_prefix("utun")
                .and_then(|s| s.parse::<u32>().ok())
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "se espera utun o utunN")
                })?
        };
        // SAFETY: llamadas con estructuras ABI inicializadas y descriptores propios; OwnedFd cierra en todo error.
        unsafe {
            let raw = libc::socket(libc::PF_SYSTEM, libc::SOCK_DGRAM, 2);
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let fd = OwnedFd::from_raw_fd(raw);
            let mut info = ControlInfo {
                id: 0,
                name: [0; 96],
            };
            let control = b"com.apple.net.utun_control";
            info.name[..control.len()].copy_from_slice(control);
            if libc::ioctl(raw, CTLIOCGINFO, &mut info) < 0 {
                return Err(io::Error::last_os_error());
            }
            let addr = ControlAddr {
                len: std::mem::size_of::<ControlAddr>() as u8,
                family: libc::AF_SYSTEM as u8,
                sysaddr: 2,
                id: info.id,
                unit,
                reserved: [0; 5],
            };
            if libc::connect(
                raw,
                (&addr as *const ControlAddr).cast(),
                u32::from(addr.len),
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut bytes = [0u8; 16];
            let mut len = bytes.len() as libc::socklen_t;
            if libc::getsockopt(raw, 2, 2, bytes.as_mut_ptr().cast(), &mut len) < 0 {
                return Err(io::Error::last_os_error());
            }
            let end = bytes.iter().position(|b| *b == 0).ok_or_else(invalid)?;
            let name = std::str::from_utf8(&bytes[..end])
                .map_err(|_| invalid())?
                .to_owned();
            if libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) < 0
                || libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                fd: AsyncFd::register(fd).map_err(io::Error::from)?,
                name,
            })
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub async fn read_packet(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut ready = self.fd.readable().await?;
            let mut header = [0u8; 4];
            // SAFETY: ambos iovec apuntan a búferes exclusivos válidos durante readv.
            let result = ready.try_io(|fd| unsafe {
                let vectors = [
                    libc::iovec {
                        iov_base: header.as_mut_ptr().cast(),
                        iov_len: 4,
                    },
                    libc::iovec {
                        iov_base: buf.as_mut_ptr().cast(),
                        iov_len: buf.len(),
                    },
                ];
                let n = libc::readv(fd.as_raw_fd(), vectors.as_ptr(), 2);
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else if n < 4 {
                    Err(invalid())
                } else {
                    normalize(header, &buf[..n as usize - 4])
                }
            });
            if let Ok(result) = result {
                return result;
            }
        }
    }
    pub async fn write_packet(&self, packet: &[u8]) -> io::Result<usize> {
        let header = family(packet)?.to_be_bytes();
        loop {
            let mut ready = self.fd.writable().await?;
            // SAFETY: writev solo lee los dos búferes inmutables durante la llamada.
            let result = ready.try_io(|fd| unsafe {
                let vectors = [
                    libc::iovec {
                        iov_base: header.as_ptr().cast_mut().cast(),
                        iov_len: 4,
                    },
                    libc::iovec {
                        iov_base: packet.as_ptr().cast_mut().cast(),
                        iov_len: packet.len(),
                    },
                ];
                let n = libc::writev(fd.as_raw_fd(), vectors.as_ptr(), 2);
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else if n as usize != packet.len() + 4 {
                    Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "escritura utun incompleta",
                    ))
                } else {
                    Ok(packet.len())
                }
            });
            if let Ok(result) = result {
                return result;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_are_network_order_and_match_ip() {
        let mut v4 = [0; 20];
        v4[0] = 0x45;
        v4[3] = 20;
        assert_eq!(normalize(2u32.to_be_bytes(), &v4).unwrap(), 20);
        assert!(normalize(30u32.to_be_bytes(), &v4).is_err());
        assert!(normalize(2u32.to_le_bytes(), &v4).is_err());
        assert!(family(&v4[..19]).is_err());
        v4[0] = 0x44;
        assert!(family(&v4).is_err());
        let mut v6 = [0; 40];
        v6[0] = 0x60;
        assert_eq!(normalize(30u32.to_be_bytes(), &v6).unwrap(), 40);
        v6[5] = 1;
        assert!(family(&v6).is_err());
        assert!(family(&[]).is_err());
        // El agente descarta estos paquetes por su tipo: solo `InvalidData`.
        assert_eq!(
            normalize(2u32.to_le_bytes(), &v4).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
