use crate::*;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV6};
pub(crate) struct Reader<'a> {
    b: &'a [u8],
}
impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b }
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (a, b) = self.b.split_at_checked(N).ok_or(Error::Malformed)?;
        self.b = b;
        Ok(a.try_into().expect("longitud comprobada"))
    }
    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    pub fn end(self) -> Result<(), Error> {
        if self.b.is_empty() {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn header(&mut self, kind: u8) -> Result<(), Error> {
        if self.u8()? != 1 || self.u8()? != kind {
            Err(Error::UnsupportedVersion)
        } else {
            Ok(())
        }
    }
    pub fn zero(&mut self) -> Result<(), Error> {
        if self.u8()? == 0 {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn count8(&mut self, min: usize, max: usize) -> Result<usize, Error> {
        let n = self.u8()? as usize;
        if (min..=max).contains(&n) {
            Ok(n)
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn count16(&mut self, max: usize) -> Result<usize, Error> {
        let n = self.u16()? as usize;
        if n <= max {
            Ok(n)
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn addr(&mut self) -> Result<IpAddr, Error> {
        match self.u8()? {
            4 => Ok(Ipv4Addr::from(self.array::<4>()?).into()),
            6 => Ok(Ipv6Addr::from(self.array::<16>()?).into()),
            _ => Err(Error::Malformed),
        }
    }
    pub fn prefix(&mut self) -> Result<Prefix, Error> {
        let p = Prefix {
            addr: self.addr()?,
            len: self.u8()?,
        };
        if p.valid() {
            Ok(p)
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn endpoint(&mut self) -> Result<SocketAddr, Error> {
        let a = self.addr()?;
        let p = self.u16()?;
        if p == 0 {
            Err(Error::Malformed)
        } else {
            Ok(SocketAddr::new(a, p))
        }
    }
    pub fn selector(&mut self) -> Result<Selector, Error> {
        match self.u8()? {
            1 => Ok(Selector::Any),
            2 => Ok(Selector::Device(self.array()?)),
            3 => Ok(Selector::Prefix(self.prefix()?)),
            _ => Err(Error::Malformed),
        }
    }
    pub fn addrs(&mut self) -> Result<Vec<IpAddr>, Error> {
        let n = self.count8(1, 4)?;
        (0..n).map(|_| self.addr()).collect()
    }
    pub fn prefixes(&mut self) -> Result<Vec<Prefix>, Error> {
        let n = self.count8(0, 8)?;
        (0..n).map(|_| self.prefix()).collect()
    }
    pub fn domain(&mut self) -> Result<String, Error> {
        let n = self.count8(1, 253)?;
        let (a, b) = self.b.split_at_checked(n).ok_or(Error::Malformed)?;
        self.b = b;
        let s = std::str::from_utf8(a).map_err(|_| Error::Malformed)?;
        if !valid_domain(s) {
            return Err(Error::Malformed);
        }
        Ok(s.to_owned())
    }
}
pub(crate) fn addr(b: &mut Vec<u8>, a: &IpAddr) {
    match a {
        IpAddr::V4(a) => {
            b.push(4);
            b.extend_from_slice(&a.octets());
        }
        IpAddr::V6(a) => {
            b.push(6);
            b.extend_from_slice(&a.octets());
        }
    }
}
pub(crate) fn prefix(b: &mut Vec<u8>, p: &Prefix) {
    addr(b, &p.addr);
    b.push(p.len);
}
pub(crate) fn endpoint(b: &mut Vec<u8>, e: &SocketAddr) {
    addr(b, &e.ip());
    b.extend_from_slice(&e.port().to_be_bytes());
}
pub(crate) fn selector(b: &mut Vec<u8>, s: &Selector) {
    match s {
        Selector::Any => b.push(1),
        Selector::Device(d) => {
            b.push(2);
            b.extend_from_slice(d);
        }
        Selector::Prefix(p) => {
            b.push(3);
            prefix(b, p);
        }
    }
}
pub(crate) fn rule(b: &mut Vec<u8>, r: &Rule) {
    selector(b, &r.src);
    selector(b, &r.dst);
    b.push(r.proto);
    b.extend_from_slice(&r.port_min.to_be_bytes());
    b.extend_from_slice(&r.port_max.to_be_bytes());
}
pub(crate) fn encoded<T>(v: &T, f: fn(&mut Vec<u8>, &T)) -> Vec<u8> {
    let mut b = Vec::new();
    f(&mut b, v);
    b
}
pub(crate) fn ordered<T>(v: &[T], f: fn(&mut Vec<u8>, &T)) -> bool {
    v.windows(2).all(|w| encoded(&w[0], f) < encoded(&w[1], f))
}
pub(crate) fn sort<T>(v: &mut [T], f: fn(&mut Vec<u8>, &T)) {
    v.sort_by_cached_key(|a| encoded(a, f));
}
pub(crate) fn endpoint_valid(e: &SocketAddr) -> bool {
    e.port() != 0
        && match e {
            SocketAddr::V4(_) => true,
            SocketAddr::V6(e) => *e == SocketAddrV6::new(*e.ip(), e.port(), 0, 0),
        }
}
impl Prefix {
    pub fn valid(&self) -> bool {
        match self.addr {
            IpAddr::V4(a) => {
                self.len <= 32
                    && (self.len == 32
                        || u32::from(a).checked_shl(self.len.into()).unwrap_or(0) == 0)
            }
            IpAddr::V6(a) => {
                self.len <= 128
                    && (self.len == 128
                        || u128::from(a).checked_shl(self.len.into()).unwrap_or(0) == 0)
            }
        }
    }
    pub fn contains(&self, a: IpAddr) -> bool {
        match (self.addr, a) {
            (IpAddr::V4(p), IpAddr::V4(a)) if self.len <= 32 => {
                self.len == 0 || (u32::from(p) ^ u32::from(a)) >> (32 - self.len) == 0
            }
            (IpAddr::V6(p), IpAddr::V6(a)) if self.len <= 128 => {
                self.len == 0 || (u128::from(p) ^ u128::from(a)) >> (128 - self.len) == 0
            }
            _ => false,
        }
    }
    /// Comprueba contención completa, incluida la familia y longitud.
    pub fn contains_prefix(&self, other: &Self) -> bool {
        self.len <= other.len && self.contains(other.addr)
    }
    pub fn overlaps(&self, other: &Self) -> bool {
        self.contains(other.addr) || other.contains(self.addr)
    }
    /// Solo subredes privadas canónicas autorizables por ADR-007.
    pub fn valid_route(&self) -> bool {
        if !self.valid() {
            return false;
        }
        match self.addr {
            IpAddr::V4(a) => {
                self.len >= 8
                    && [
                        (std::net::Ipv4Addr::new(10, 0, 0, 0), 8),
                        (std::net::Ipv4Addr::new(172, 16, 0, 0), 12),
                        (std::net::Ipv4Addr::new(192, 168, 0, 0), 16),
                        (std::net::Ipv4Addr::new(100, 64, 0, 0), 10),
                    ]
                    .iter()
                    .any(|(addr, len)| {
                        Prefix {
                            addr: (*addr).into(),
                            len: *len,
                        }
                        .contains_prefix(self)
                    })
                    && !a.is_unspecified()
            }
            IpAddr::V6(a) => self.len >= 32 && a.octets()[0] & 0xfe == 0xfc,
        }
    }
}
