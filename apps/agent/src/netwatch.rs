// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Despertar y cambio de red, por muestreo (ADR-030).
//!
//! Sin dependencias nuevas ni código `unsafe`: no se escucha al sistema operativo, se comparan dos muestras cada segundo.
//! - **Despertar:** el reloj de pared avanza más que el monótono. `Instant` no cuenta el tiempo suspendido en macOS ni en
//!   Linux, así que la diferencia es lo que duró el sueño. En Windows no se ha comprobado que `Instant` se comporte igual.
//! - **Cambio de red:** la dirección de origen que el sistema elegiría hacia un destino de documentación (RFC 5737 y
//!   RFC 3849) cambió. `connect` en UDP solo consulta la tabla de rutas: no envía nada. Si dos redes dan la misma dirección
//!   privada, el cambio no se ve (queda el despertar y los plazos de siempre).
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

/// Diferencia mínima entre ambos relojes para considerar que el equipo estuvo suspendido: el muestreo es de 1 s, así que en
/// funcionamiento normal es ~0 aunque el proceso vaya con retraso (ambos relojes avanzan igual).
const WAKE_GAP: Duration = Duration::from_secs(5);

/// Dirección de origen que usaría el sistema hacia fuera, por familia; `None` si esa familia no tiene ruta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Route {
    v4: Option<IpAddr>,
    v6: Option<IpAddr>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// El equipo estuvo suspendido este tiempo.
    Wake(Duration),
    /// La ruta de salida cambió.
    Network,
}

pub struct NetWatch {
    mono: Instant,
    wall: SystemTime,
    route: Route,
}

impl NetWatch {
    pub fn new(mono: Instant, wall: SystemTime, route: Route) -> NetWatch {
        NetWatch { mono, wall, route }
    }

    /// Compara con la muestra anterior. El despertar gana: reinicia también los caminos. Quedarse sin ninguna ruta no es un
    /// evento (no hay a dónde reconectar); volver a tenerla sí, aunque sea la misma dirección que antes.
    pub fn sample(&mut self, mono: Instant, wall: SystemTime, route: Route) -> Option<Event> {
        // Un reloj de pared que retrocede (ajuste manual o NTP) no es un despertar.
        let slept = wall
            .duration_since(self.wall)
            .ok()
            .and_then(|w| w.checked_sub(mono.saturating_duration_since(self.mono)))
            .filter(|d| *d >= WAKE_GAP);
        let changed = route != self.route && route != Route::default();
        *self = NetWatch { mono, wall, route };
        match (slept, changed) {
            (Some(slept), _) => Some(Event::Wake(slept)),
            (None, true) => Some(Event::Network),
            (None, false) => None,
        }
    }
}

/// Ruta de salida actual. Solo consulta la tabla de rutas del sistema: son llamadas al sistema, sin red ni procesos.
pub fn route_now() -> Route {
    Route {
        v4: source_for(SocketAddr::from(([192, 0, 2, 1], 9))),
        v6: source_for(SocketAddr::from(([0x2001, 0x0db8, 0, 0, 0, 0, 0, 1], 9))),
    }
}

fn source_for(destination: SocketAddr) -> Option<IpAddr> {
    let any = if destination.is_ipv4() {
        SocketAddr::from(([0, 0, 0, 0], 0))
    } else {
        SocketAddr::from(([0u16; 8], 0))
    };
    let socket = UdpSocket::bind(any).ok()?;
    socket.connect(destination).ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(v4: Option<&str>, v6: Option<&str>) -> Route {
        Route {
            v4: v4.map(|a| a.parse().unwrap()),
            v6: v6.map(|a| a.parse().unwrap()),
        }
    }

    struct Clocks {
        mono: Instant,
        wall: SystemTime,
    }

    impl Clocks {
        fn new() -> Clocks {
            Clocks {
                mono: Instant::now(),
                wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }
        }
        /// Pasa `awake` segundos despierto y `asleep` suspendido: el monótono solo cuenta los primeros.
        fn pass(&mut self, awake: u64, asleep: u64) {
            self.mono += Duration::from_secs(awake);
            self.wall += Duration::from_secs(awake + asleep);
        }
    }

    fn watch(c: &Clocks, r: Route) -> NetWatch {
        NetWatch::new(c.mono, c.wall, r)
    }

    #[test]
    fn nothing_changes_nothing_happens() {
        let mut c = Clocks::new();
        let r = route(Some("192.168.1.20"), Some("2001:db8:1::20"));
        let mut w = watch(&c, r);
        for _ in 0..30 {
            c.pass(1, 0);
            assert_eq!(w.sample(c.mono, c.wall, r), None);
        }
    }

    #[test]
    fn a_suspension_is_a_wake_with_its_length() {
        let mut c = Clocks::new();
        let r = route(Some("192.168.1.20"), None);
        let mut w = watch(&c, r);
        c.pass(1, 600);
        assert_eq!(
            w.sample(c.mono, c.wall, r),
            Some(Event::Wake(Duration::from_secs(600)))
        );
        // Una sola vez: la muestra siguiente ya parte del despertar.
        c.pass(1, 0);
        assert_eq!(w.sample(c.mono, c.wall, r), None);
    }

    #[test]
    fn a_short_pause_or_a_late_process_is_not_a_wake() {
        let mut c = Clocks::new();
        let r = route(Some("192.168.1.20"), None);
        let mut w = watch(&c, r);
        // El proceso fue 30 s tarde con el equipo despierto (carga): ambos relojes avanzan igual.
        c.pass(30, 0);
        assert_eq!(w.sample(c.mono, c.wall, r), None);
        // Una suspensión de 3 s queda por debajo del umbral.
        c.pass(1, 3);
        assert_eq!(w.sample(c.mono, c.wall, r), None);
    }

    #[test]
    fn a_wall_clock_that_goes_back_is_not_a_wake() {
        let mut c = Clocks::new();
        let r = route(Some("192.168.1.20"), None);
        let mut w = watch(&c, r);
        c.mono += Duration::from_secs(1);
        c.wall -= Duration::from_secs(3600);
        assert_eq!(w.sample(c.mono, c.wall, r), None);
    }

    #[test]
    fn a_new_source_address_is_a_network_change_in_either_family() {
        let mut c = Clocks::new();
        let mut w = watch(&c, route(Some("192.168.1.20"), Some("2001:db8:1::20")));
        c.pass(1, 0);
        assert_eq!(
            w.sample(
                c.mono,
                c.wall,
                route(Some("10.0.0.7"), Some("2001:db8:1::20"))
            ),
            Some(Event::Network)
        );
        c.pass(1, 0);
        assert_eq!(
            w.sample(
                c.mono,
                c.wall,
                route(Some("10.0.0.7"), Some("2001:db8:2::9"))
            ),
            Some(Event::Network)
        );
        c.pass(1, 0);
        assert_eq!(
            w.sample(c.mono, c.wall, route(Some("10.0.0.7"), None)),
            Some(Event::Network)
        );
    }

    #[test]
    fn losing_every_route_is_silent_and_getting_one_back_is_an_event() {
        let mut c = Clocks::new();
        let home = route(Some("192.168.1.20"), None);
        let mut w = watch(&c, home);
        c.pass(1, 0);
        assert_eq!(w.sample(c.mono, c.wall, Route::default()), None);
        c.pass(1, 0);
        // Mismo Wi-Fi, misma dirección: el sistema estuvo sin red y los caminos no se pueden dar por buenos.
        assert_eq!(w.sample(c.mono, c.wall, home), Some(Event::Network));
    }

    #[test]
    fn a_wake_on_a_new_network_is_reported_once_as_a_wake() {
        let mut c = Clocks::new();
        let mut w = watch(&c, route(Some("192.168.1.20"), None));
        c.pass(1, 7200);
        assert_eq!(
            w.sample(c.mono, c.wall, route(Some("10.0.0.7"), None)),
            Some(Event::Wake(Duration::from_secs(7200)))
        );
        c.pass(1, 0);
        assert_eq!(
            w.sample(c.mono, c.wall, route(Some("10.0.0.7"), None)),
            None
        );
    }

    #[test]
    fn the_live_probe_never_fails_and_is_stable() {
        // Sin red (contenedor de CI) todo es `None`; con red, dos lecturas seguidas dan lo mismo.
        assert_eq!(route_now(), route_now());
    }
}
