use axum::extract::State;
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

pub async fn login(
    State(state): State<AppState>,
    cookies: Cookies,
    Json(req): Json<LoginRequest>,
) -> Response {
    if state.verify_password(&req.password) {
        let cookie = Cookie::build((SESSION_COOKIE, "1"))
            .path("/")
            .http_only(true)
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
