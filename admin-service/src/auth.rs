use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// Auth configuration shared across the router. Built once in main.rs from
/// `LAGRANGE_TOKEN_FILE` + `LAGRANGE_TRUSTED_SSO_PEER`.
pub struct AuthCfg {
    pub expected_token: Arc<str>,
    /// When set, requests whose source IP equals this value are also allowed
    /// if they carry an `X-authentik-username` header — that path lets the
    /// cluster-side ingress-nginx forward authentik-SSO sessions to the
    /// admin API without us minting a second bearer-token Secret. The IP
    /// gate is the trust signal: only the cluster wg-gateway can originate
    /// traffic with the wg peer address as its source.
    pub trusted_sso_peer: Option<IpAddr>,
}

const SSO_USER_HEADER: &str = "x-authentik-username";

/// Outcome of an auth check. Extracted so unit tests don't have to build a
/// full axum Router to exercise the policy.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthDecision {
    Bearer,
    Sso { username: String },
    Reject,
}

pub fn decide(headers: &HeaderMap, peer_ip: Option<IpAddr>, cfg: &AuthCfg) -> AuthDecision {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    if let Some(presented) = presented {
        if constant_time_eq(presented.as_bytes(), cfg.expected_token.as_bytes()) {
            return AuthDecision::Bearer;
        }
    }

    if let (Some(trusted), Some(peer)) = (cfg.trusted_sso_peer, peer_ip) {
        if peer == trusted {
            if let Some(user) = headers
                .get(SSO_USER_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return AuthDecision::Sso {
                    username: user.to_string(),
                };
            }
        }
    }

    AuthDecision::Reject
}

pub async fn require_auth(
    cfg: Arc<AuthCfg>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let peer_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    match decide(req.headers(), peer_ip, &cfg) {
        AuthDecision::Bearer => Ok(next.run(req).await),
        AuthDecision::Sso { username } => {
            tracing::info!(user = %username, peer = ?peer_ip, "authenticated via SSO");
            Ok(next.run(req).await)
        }
        AuthDecision::Reject => Err(StatusCode::UNAUTHORIZED),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    // Fixed-length tokens (sourced from sops), so the length-mismatch early
    // return doesn't leak useful timing in practice. Still constant-time
    // within equal-length inputs.
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn cfg(token: &str, trusted_sso_peer: Option<&str>) -> AuthCfg {
        AuthCfg {
            expected_token: Arc::from(token),
            trusted_sso_peer: trusted_sso_peer.map(|s| s.parse().unwrap()),
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn correct_bearer_is_accepted() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("authorization", "Bearer s3cret")]);
        assert_eq!(decide(&h, None, &c), AuthDecision::Bearer);
    }

    #[test]
    fn wrong_bearer_is_rejected() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("authorization", "Bearer not-it")]);
        assert_eq!(decide(&h, None, &c), AuthDecision::Reject);
    }

    #[test]
    fn missing_auth_is_rejected() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        assert_eq!(decide(&HeaderMap::new(), None, &c), AuthDecision::Reject);
    }

    #[test]
    fn sso_from_trusted_peer_is_accepted() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("x-authentik-username", "chris")]);
        let peer = Some("10.99.0.1".parse().unwrap());
        assert_eq!(
            decide(&h, peer, &c),
            AuthDecision::Sso {
                username: "chris".to_string(),
            }
        );
    }

    #[test]
    fn sso_from_other_peer_is_rejected() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("x-authentik-username", "chris")]);
        let peer = Some("10.99.0.99".parse().unwrap());
        assert_eq!(decide(&h, peer, &c), AuthDecision::Reject);
    }

    #[test]
    fn sso_without_peer_extension_is_rejected() {
        // Defensive: if into_make_service_with_connect_info isn't wired, the
        // extension is absent and peer_ip is None. We must refuse the SSO
        // path rather than silently default to "trusted".
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("x-authentik-username", "chris")]);
        assert_eq!(decide(&h, None, &c), AuthDecision::Reject);
    }

    #[test]
    fn empty_username_is_rejected() {
        let c = cfg("s3cret", Some("10.99.0.1"));
        let h = headers(&[("x-authentik-username", "   ")]);
        let peer = Some("10.99.0.1".parse().unwrap());
        assert_eq!(decide(&h, peer, &c), AuthDecision::Reject);
    }

    #[test]
    fn sso_disabled_in_config_rejects_even_from_trusted_peer() {
        let c = cfg("s3cret", None);
        let h = headers(&[("x-authentik-username", "chris")]);
        let peer = Some("10.99.0.1".parse().unwrap());
        assert_eq!(decide(&h, peer, &c), AuthDecision::Reject);
    }

    #[test]
    fn bearer_still_works_when_sso_disabled() {
        let c = cfg("s3cret", None);
        let h = headers(&[("authorization", "Bearer s3cret")]);
        assert_eq!(decide(&h, None, &c), AuthDecision::Bearer);
    }
}
