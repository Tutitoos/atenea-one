use crate::ApiError;
use atenea_identity::{
    DeviceId, PublicKey,
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
    entries: HashMap<(DeviceId, [u8; 16]), u64>,
    max: usize,
}
impl Nonces {
    pub fn new(max: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max,
        }
    }
    pub fn verify(
        &mut self,
        req: &SignedRequest,
        key: &PublicKey,
        method: &Method,
        uri: &Uri,
        body: &[u8],
        now: u64,
    ) -> Result<(), ApiError> {
        api_auth::verify_request(
            req,
            key,
            method.as_str(),
            uri.path_and_query().map_or(uri.path(), |v| v.as_str()),
            body,
        )
        .map_err(|_| ApiError::auth())?;
        self.entries.retain(|_, end| *end > now);
        let k = (req.device_id, req.nonce);
        if self.entries.contains_key(&k) || self.entries.len() >= self.max {
            return Err(ApiError::auth());
        }
        self.entries.insert(k, now.saturating_add(120));
        Ok(())
    }
}
