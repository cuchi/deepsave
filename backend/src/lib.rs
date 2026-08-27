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
use axum::extract::{DefaultBodyLimit, State};
use axum::middleware;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_cookies::{CookieManagerLayer, Key};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::info;

use services::ai::AiClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub session_key: Key,
    pub storage_dir: PathBuf,
    pub ai: AiClient,
    pub coverage_months: u32,
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
    tokio::spawn(services::queue::run_worker(pool.clone(), ai.clone()));
    tokio::spawn(services::ai_tags::run_worker(pool.clone(), ai.clone()));
    tokio::spawn(services::sources::backfill_null_sources(pool.clone()));
    {
        let pool = pool.clone();
        let storage_dir = storage_dir.clone();
        tokio::spawn(async move {
            // Idempotent: re-parses only documents with unassigned installment items.
            let _ = services::series::backfill(&pool, &storage_dir).await;
        });
    }
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let _ = services::ingest::reclassify_pix_as_internal(&pool).await;
        });
    }

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
        .route("/items/{id}/apply-memory", post(routes::items::apply_memory))
        .route("/items/{id}/accept-suggestion", post(routes::items::accept_suggestion))
        .route("/memory", get(routes::memory::list_memory).post(routes::memory::create_memory))
        .route(
            "/memory/{id}",
            patch(routes::memory::update_memory).delete(routes::memory::delete_memory),
        )
        .route("/memory/preview", post(routes::memory::preview))
        .route("/memory/apply", post(routes::memory::apply))
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
        .route(
            "/documents",
            get(routes::documents::list)
                .post(routes::documents::upload)
                .layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .route(
            "/documents/{id}",
            get(routes::documents::get).delete(routes::documents::delete),
        )
        .route("/documents/{id}/file", get(routes::documents::file))
        .route("/documents/{id}/reprocess", post(routes::documents::reprocess))
        .route("/matches", get(routes::matches::list))
        .route("/matches/suggest", post(routes::matches::suggest))
        .route("/matches/{id}/accept", post(routes::matches::accept))
        .route("/matches/{id}/reject", post(routes::matches::reject))
        .route("/dashboard", get(routes::dashboard::dashboard))
        .route("/dashboard/trend", get(routes::dashboard::trend))
        .route("/dashboard/daily", get(routes::dashboard::daily))
        .route("/dashboard/tags", get(routes::dashboard::tags))
        .route("/dashboard/expected", get(routes::dashboard::expected))
        .route("/sources", get(routes::sources::list))
        .route("/sources/{id}", patch(routes::sources::update))
        .route("/coverage", get(routes::sources::coverage))
        .route("/system", get(routes::system::system))
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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
        .fallback_service(serve_dir);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT 1::bigint")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    Json(json!({ "status": "ok", "db": db_ok }))
}
