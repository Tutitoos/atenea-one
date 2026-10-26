// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use atenea_identity::{DeviceConfig, DeviceId};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::Notify;

#[derive(Clone, Serialize)]
pub struct RelayGrant {
    #[serde(flatten)]
    pub relay: crate::config::Relay,
    pub token: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Connect {
        peer: String,
        candidates: Vec<SocketAddr>,
        seq: u64,
    },
    Relay {
        peer: String,
        relays: Vec<RelayGrant>,
        ttl_secs: u64,
        seq: u64,
    },
}
pub(crate) struct Presence {
    pub config: DeviceConfig,
    pub candidates: Vec<SocketAddr>,
    pub expires: u64,
    pub events: VecDeque<(u64, Event)>,
    pub notify: Arc<Notify>,
}
impl Presence {
    pub fn drain(&mut self, now: u64) -> Vec<Event> {
        self.events
            .drain(..)
            .filter(|(end, _)| *end > now)
            .map(|(_, e)| e)
            .collect()
    }
    pub fn enqueue(&mut self, event: Event, now: u64) {
        self.events.retain(|(end, _)| *end > now);
        if self.events.len() == 16 {
            self.events.pop_front();
        }
        self.events.push_back((now.saturating_add(30), event));
        self.notify.notify_waiters();
    }
}
pub(crate) fn sweep(presences: &mut HashMap<DeviceId, Presence>, now: u64) {
    presences.retain(|_, p| {
        p.events.retain(|(end, _)| *end > now);
        if p.expires <= now {
            p.notify.notify_waiters();
            false
        } else {
            true
        }
    });
}
