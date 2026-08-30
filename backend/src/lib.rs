pub mod auth;
pub mod config;
pub mod error;
pub mod models;
pub mod routes;
pub mod services;

use anyhow::Context;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::State;
use axum::middleware;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_cookies::{CookieManagerLayer, Key};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::routes::auth::LoginLimiter;
use crate::config::PluggyAccountConf;
use services::ai::AiClient;
use services::pluggy::PluggyClient;

/// Optional Pluggy client — present only when the env is configured.
#[derive(Clone, Default)]
pub struct PluggyHandle {
    inner: Option<PluggyClient>,
}

impl PluggyHandle {
    pub fn is_configured(&self) -> bool {
        self.inner.as_ref().map_or(false, |c| c.is_configured())
    }
    pub fn client(&self) -> Option<&PluggyClient> {
        self.inner.as_ref()
    }
    /// 'api_key' when PLUGGY_API_KEY is used, 'client' when the /auth flow is.
    pub fn auth_mode(&self) -> &'static str {
        match self.inner.as_ref() {
            Some(c) if c.uses_fixed_key() => "api_key",
            Some(_) => "client",
            None => "none",
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub session_key: Key,
    pub storage_dir: PathBuf,
    pub ai: AiClient,
    pub coverage_months: u32,
    /// Set the session cookie `Secure` flag (requires HTTPS at the reverse proxy).
    pub cookie_secure: bool,
    /// In-memory brute-force guard for the login endpoint.
    pub login_limiter: Arc<LoginLimiter>,
    pub pluggy: PluggyHandle,
    /// Accounts to sync (from `PLUGGY_ACCOUNTS`).
    pub pluggy_accounts: Vec<PluggyAccountConf>,
    password_hash: String,
}

impl AppState {
    pub fn verify_password(&self, candidate: &str) -> bool {
        match PasswordHash::new(&self.password_hash) {
            Ok(parsed) => Argon2::default()
                .verify_password(candidate.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    }
}

fn resolve_password_hash(config: &config::Config) -> anyhow::Result<String> {
    if let Some(hash) = &config.password_hash {
        return Ok(hash.clone());
    }
    let plain = config
        .password_plain
        .clone()
        .unwrap_or_else(|| "deepsave".to_string());
    tracing::warn!(
        "APP_PASSWORD_HASH not set; hashing APP_PASSWORD at startup (default 'deepsave'). \
         Set APP_PASSWORD_HASH in production."
    );
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)?
        .to_string();
    Ok(hash)
}

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "deepsave_backend=info,tower_http=info".into()),
        )
        .init();

    let config = config::Config::from_env()?;
    let password_hash = resolve_password_hash(&config)?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
        .context("failed to connect to postgres")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run migrations")?;
    info!("migrations applied");

    let storage_dir = PathBuf::from(&config.storage_dir);
    tokio::fs::create_dir_all(&storage_dir)
        .await
        .context("failed to create storage dir")?;
    let storage_dir = tokio::fs::canonicalize(&storage_dir)
        .await
        .context("failed to resolve storage dir")?;

    let ai = AiClient::new(&config, pool.clone());
    // Prefer a fixed API key (single-user): no /auth round-trip, no expiry
    // handling. Fall back to client credentials only when the key is absent.
    let pluggy = if let Some(key) = &config.pluggy_api_key {
        PluggyHandle {
            inner: Some(PluggyClient::from_api_key(key.clone())),
        }
    } else if let (Some(id), Some(secret)) = (&config.pluggy_client_id, &config.pluggy_client_secret) {
        PluggyHandle {
            inner: Some(PluggyClient::new(id.clone(), secret.clone())),
        }
    } else {
        tracing::warn!("PLUGGY_API_KEY (or PLUGGY_CLIENT_ID/PLUGGY_CLIENT_SECRET) not set — pluggy integration disabled");
        PluggyHandle::default()
    };
    // Seed the configured accounts so the UI can list them before the first sync.
    if pluggy.is_configured() {
        let seeded = services::pluggy::seed_configured_accounts(&pool, &config.pluggy_accounts).await;
        match seeded {
            Ok(n) if n > 0 => info!("pluggy: {n} account(s) configured"),
            Ok(_) => tracing::warn!("PLUGGY_ACCOUNTS is empty — nothing to sync"),
            Err(e) => tracing::warn!("pluggy seed failed: {e:#}"),
        }
    } else if !config.pluggy_accounts.is_empty() {
        tracing::warn!("PLUGGY_ACCOUNTS set but Pluggy credentials missing — integration disabled");
    }
    tokio::spawn(services::ai_tags::run_worker(pool.clone(), ai.clone()));

    let session_key = if config.session_secret.len() >= 32 {
        Key::derive_from(config.session_secret.as_bytes())
    } else {
        anyhow::bail!("SESSION_SECRET must be at least 32 bytes");
    };

    let state = AppState {
        pool,
        session_key,
        storage_dir,
        password_hash,
        ai,
        coverage_months: config.coverage_months,
        cookie_secure: config.cookie_secure,
        login_limiter: Arc::new(LoginLimiter::default()),
        pluggy,
        pluggy_accounts: config.pluggy_accounts,
    };

    let protected = Router::new()
        .route(
            "/categories",
            get(routes::categories::list).post(routes::categories::create),
        )
        .route(
            "/categories/{id}",
            patch(routes::categories::update).delete(routes::categories::delete),
        )
        .route(
            "/items",
            get(routes::items::list).post(routes::items::create),
        )
        .route("/items/bulk", patch(routes::items::bulk_update))
        .route("/items/link-recurring", post(routes::items::bulk_link_recurring))
        .route("/items/summary", get(routes::items::items_summary))
        .route("/tags", get(routes::tags::list))
        .route("/tags/usage", get(routes::tags::usage))
        .route("/tags/registry", get(routes::tags::registry))
        .route("/tags/{tag}", patch(routes::tags::set_description))
        .route("/change-log", get(routes::change_log::list))
        .route(
            "/diary",
            get(routes::diary::list).post(routes::diary::create),
        )
        .route("/diary/{id}", patch(routes::diary::update).delete(routes::diary::delete))
        .route("/tags/rename", patch(routes::tags::rename))
        .route("/tags/merge", post(routes::tags::merge))
        .route("/tags/{tag}", delete(routes::tags::delete_tag))
        .route("/ai-tags/batches", post(routes::ai_tags::create_batch).get(routes::ai_tags::list_batches))
        .route("/ai-tags/suggestions", get(routes::ai_tags::list_suggestions))
        .route("/ai-tags/suggestions/{id}/apply", post(routes::ai_tags::apply))
        .route("/ai-tags/suggestions/{id}/dismiss", post(routes::ai_tags::dismiss))
        .route("/ai-tags/suggestions/apply-all", post(routes::ai_tags::apply_all))
        .route("/ai-tags/suggestions/dismiss-all", post(routes::ai_tags::dismiss_all))
        .route(
            "/items/{id}",
            get(routes::items::get)
                .patch(routes::items::update)
                .delete(routes::items::delete),
        )
        .route("/items/{id}/confirm", post(routes::items::confirm))
        .route("/items/{id}/reject", post(routes::items::reject))
        .route("/items/{id}/link-recurring", post(routes::items::link_recurring))
        .route("/items/{id}/accept-suggestion", post(routes::items::accept_suggestion))
        .route("/banks", get(routes::items::banks))
        .route(
            "/recurring",
            get(routes::recurring::list).post(routes::recurring::create),
        )
        .route(
            "/recurring/{id}",
            patch(routes::recurring::update).delete(routes::recurring::delete),
        )
        .route("/recurring/{id}/occurrences", get(routes::recurring::occurrences))
        .route("/recurring/merchants", get(routes::recurring::merchants))
        .route("/recurring/merchant-profile", get(routes::recurring::merchant_profile))
        .route("/recurring/monthly-cost", get(routes::recurring::monthly_cost))
        .route("/dashboard", get(routes::dashboard::dashboard))
        .route("/dashboard/trend", get(routes::dashboard::trend))
        .route("/dashboard/daily", get(routes::dashboard::daily))
        .route("/dashboard/tags", get(routes::dashboard::tags))
        .route("/dashboard/expected", get(routes::dashboard::expected))
        .route("/dashboard/forecast", get(routes::dashboard::forecast))
        .route("/dashboard/upcoming", get(routes::dashboard::upcoming))
        .route(
            "/dashboard/digest",
            get(routes::dashboard::digest_get)
                .post(routes::dashboard::digest_post)
                .delete(routes::dashboard::digest_delete),
        )
        .route("/system", get(routes::system::system))
        .route("/pluggy/status", get(routes::pluggy::status))
        .route("/pluggy/accounts", get(routes::pluggy::accounts))
        .route("/pluggy/sync", post(routes::pluggy::sync))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let serve_dir = ServeDir::new(&config.static_dir)
        .fallback(ServeFile::new(format!("{}/index.html", config.static_dir)));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me", get(routes::auth::me))
        .nest("/api", protected)
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
        .fallback_service(serve_dir);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT 1::bigint")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    Json(json!({ "status": "ok", "db": db_ok }))
}
