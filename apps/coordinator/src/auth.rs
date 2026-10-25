use crate::ApiError;
use atenea_identity::{
    DeviceId, NetworkId, PublicKey,
    api_auth::{self, SignedRequest},
};
use axum::http::{HeaderMap, Method, Uri};
use std::collections::HashMap;

pub(crate) fn parse(headers: &HeaderMap, now: u64) -> Result<SignedRequest, ApiError> {
    if headers.get_all("authorization").iter().count() != 1 {
        return Err(ApiError::auth());
    }
    let req = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| api_auth::parse_authorization(v).ok())
        .ok_or_else(ApiError::auth)?;
    if req.ts.abs_diff(now) > 60 {
        return Err(ApiError::auth());
    }
    Ok(req)
}
pub(crate) struct Nonces {
    entries: HashMap<(DeviceId, [u8; 16]), (NetworkId, u64)>,
    device_counts: HashMap<DeviceId, usize>,
    network_counts: HashMap<NetworkId, usize>,
    device_rates: crate::rate::Rates<DeviceId>,
    network_rates: crate::rate::Rates<NetworkId>,
    swept_at: u64,
    max: usize,
}
impl Nonces {
    pub fn new(max: usize) -> Self {
        Self {
            entries: HashMap::new(),
            device_counts: HashMap::new(),
            network_counts: HashMap::new(),
            device_rates: crate::rate::Rates::new(max),
            network_rates: crate::rate::Rates::new(max),
            swept_at: u64::MAX,
            max,
        }
    }
    pub fn verify(
        &mut self,
        req: &SignedRequest,
        network: NetworkId,
        key: &PublicKey,
        target: (&Method, &Uri),
        body: &[u8],
        now: u64,
    ) -> Result<(), ApiError> {
        api_auth::verify_request(
            req,
            key,
            target.0.as_str(),
            target
                .1
                .path_and_query()
                .map_or(target.1.path(), |v| v.as_str()),
            body,
        )
        .map_err(|_| ApiError::auth())?;
        if !self.device_rates.allow(req.device_id, now, 1.0, 16)
            || !self.network_rates.allow(network, now, 8.0, 64)
        {
            return Err(ApiError::rate());
        }
        if self.swept_at != now {
            self.swept_at = now;
            self.entries.retain(|(device, _), (network, end)| {
                if *end > now {
                    return true;
                }
                *self
                    .device_counts
                    .get_mut(device)
                    .expect("cuota registrada") -= 1;
                *self
                    .network_counts
                    .get_mut(network)
                    .expect("cuota registrada") -= 1;
                false
            });
            self.device_counts.retain(|_, count| *count > 0);
            self.network_counts.retain(|_, count| *count > 0);
        }
        let k = (req.device_id, req.nonce);
        if self.entries.contains_key(&k) {
            return Err(ApiError::auth());
        }
        // Los nonces vigentes no se expulsan: conservan la protección antirrepetición.
        if self.entries.len() >= self.max
            || self.device_counts.get(&req.device_id).copied().unwrap_or(0)
                >= self.max.div_ceil(4).min(256)
            || self.network_counts.get(&network).copied().unwrap_or(0)
                >= self.max.div_ceil(2).min(4096)
        {
            return Err(ApiError::rate());
        }
        self.entries.insert(k, (network, now.saturating_add(120)));
        *self.device_counts.entry(req.device_id).or_default() += 1;
        *self.network_counts.entry(network).or_default() += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_nonces_are_preserved_at_capacity_and_expired_nonces_are_reclaimed() {
        let key = atenea_identity::signing_key([3; 32]);
        let public = key.verifying_key().to_bytes();
        let mut nonces = Nonces::new(8);
        let method = Method::GET;
        let uri: Uri = "/v1/events?wait=0".parse().unwrap();
        let request = |nonce, ts| {
            api_auth::parse_authorization(&api_auth::sign_request(
                &key,
                [3; 16],
                "GET",
                "/v1/events?wait=0",
                &[],
                ts,
                [nonce; 16],
            ))
            .unwrap()
        };
        for n in [1, 2] {
            nonces
                .verify(
                    &request(n, 1000),
                    [4; 16],
                    &public,
                    (&method, &uri),
                    &[],
                    1000,
                )
                .unwrap();
        }
        let full = nonces
            .verify(
                &request(3, 1000),
                [4; 16],
                &public,
                (&method, &uri),
                &[],
                1000,
            )
            .unwrap_err();
        assert_eq!(full.0, axum::http::StatusCode::TOO_MANY_REQUESTS);
        let replay = nonces
            .verify(
                &request(1, 1000),
                [4; 16],
                &public,
                (&method, &uri),
                &[],
                1060,
            )
            .unwrap_err();
        assert_eq!(replay.0, axum::http::StatusCode::UNAUTHORIZED);
        nonces
            .verify(
                &request(3, 1120),
                [4; 16],
                &public,
                (&method, &uri),
                &[],
                1120,
            )
            .unwrap();
        assert_eq!(nonces.entries.len(), 1);
        assert_eq!(nonces.device_counts[&[3; 16]], 1);
        assert_eq!(nonces.network_counts[&[4; 16]], 1);
    }
}
