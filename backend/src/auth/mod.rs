use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tower_cookies::Cookies;

use crate::AppState;

pub const SESSION_COOKIE: &str = "deepsave_session";

pub fn is_authenticated(cookies: &Cookies, state: &AppState) -> bool {
    cookies
        .signed(&state.session_key)
        .get(SESSION_COOKIE)
        .is_some()
}

/// Middleware protecting authenticated routes. Requires `CookieManagerLayer`
/// to be applied outside (before) this middleware so the `Cookies` extension
/// is present in request extensions.
pub async fn require_auth(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let (mut parts, body) = request.into_parts();

    let cookies = match Cookies::from_request_parts(&mut parts, &()).await {
        Ok(cookies) => cookies,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response()
        }
    };

    if !is_authenticated(&cookies, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }

    let request = axum::extract::Request::from_parts(parts, body);
    next.run(request).await
}
