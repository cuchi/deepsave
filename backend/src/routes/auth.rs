use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tower_cookies::cookie::{Cookie, SameSite};
use tower_cookies::Cookies;

use crate::auth::{is_authenticated, SESSION_COOKIE};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

/// In-memory brute-force guard for the login endpoint.
///
/// Counts *failed* attempts per peer IP (reset on success). Behind the VPS
/// nginx every connection comes from the tunnel peer (10.77.0.1), so this
/// effectively caps total failed logins per minute — complementing nginx's
/// per-client-IP `limit_req` at the edge (see deploy/vps/nginx-deepsave.conf).
#[derive(Default)]
pub struct LoginLimiter {
    inner: Mutex<HashMap<SocketAddr, (Instant, u32)>>,
}

/// Max failed attempts per peer per window.
const MAX_FAILURES: u32 = 20;
const WINDOW: Duration = Duration::from_secs(60);

impl LoginLimiter {
    /// Records a failed attempt; returns `true` if the peer is now over the limit.
    pub fn record_failure(&self, addr: SocketAddr) -> bool {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);

        match map.get_mut(&addr) {
            Some((start, count)) => {
                if now.duration_since(*start) >= WINDOW {
                    *start = now;
                    *count = 1;
                    false
                } else {
                    *count += 1;
                    *count > MAX_FAILURES
                }
            }
            None => {
                map.insert(addr, (now, 1));
                false
            }
        }
    }

    /// Clears the counter after a successful login.
    pub fn reset(&self, addr: SocketAddr) {
        self.inner.lock().unwrap().remove(&addr);
    }
}

pub async fn login(
    State(state): State<AppState>,
    cookies: Cookies,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Response {
    if state.login_limiter.record_failure(addr) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "too many attempts, try again later" })),
        )
            .into_response();
    }

    if state.verify_password(&req.password) {
        state.login_limiter.reset(addr);
        let cookie = Cookie::build((SESSION_COOKIE, "1"))
            .path("/")
            .http_only(true)
            .secure(state.cookie_secure)
            .same_site(SameSite::Lax)
            .build();
        cookies.signed(&state.session_key).add(cookie);
        (StatusCode::OK, Json(json!({ "authenticated": true }))).into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid password" })),
        )
            .into_response()
    }
}

pub async fn logout(cookies: Cookies) -> impl IntoResponse {
    cookies.remove(Cookie::new(SESSION_COOKIE, ""));
    (StatusCode::OK, Json(json!({ "ok": true })))
}

pub async fn me(State(state): State<AppState>, cookies: Cookies) -> Json<serde_json::Value> {
    Json(json!({ "authenticated": is_authenticated(&cookies, &state) }))
}
