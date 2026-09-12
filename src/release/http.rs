use super::{Context, Result, check_cancel};
use std::time::Duration;

pub(super) fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}
pub(super) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub(super) fn exists(cx: &Context<'_>, url: &str) -> Result<bool> {
    check_cancel(cx)?;
    let response = cx
        .http
        .get(url)
        .header(
            "User-Agent",
            "shipwright (https://github.com/jamiedavenport/shipwright)",
        )
        .call()
        .map_err(|_| "Registry lookup failed; check connectivity")?;
    match response.status().as_u16() {
        200 => Ok(true),
        404 => Ok(false),
        status => Err(format!(
            "Registry lookup failed (HTTP {status}); version absence was not established"
        )),
    }
}
