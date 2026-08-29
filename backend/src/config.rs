use anyhow::Context;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    pub session_secret: String,
    /// Pre-hashed argon2 password (from `APP_PASSWORD_HASH`).
    pub password_hash: Option<String>,
    /// Plaintext password (from `APP_PASSWORD`), hashed at startup if no hash is set.
    pub password_plain: Option<String>,
    /// Set the session cookie `Secure` flag (requires HTTPS at the reverse proxy).
    pub cookie_secure: bool,
    /// Directory containing the built frontend to serve (SPA).
    pub static_dir: String,
    /// Directory where uploaded documents are stored.
    pub storage_dir: String,
    /// Coverage window (months), e.g. 12.
    pub coverage_months: u32,

    // DeepSeek
    pub deepseek_api_key: Option<String>,
    pub deepseek_base_url: String,
    pub deepseek_model: String,
    pub deepseek_vision_model: String,
    pub deepseek_pro_model: String,
    pub deepseek_input_price_per_m: f64,
    pub deepseek_cache_hit_price_per_m: f64,
    pub deepseek_output_price_per_m: f64,

    // Pluggy
    pub pluggy_client_id: Option<String>,
    pub pluggy_client_secret: Option<String>,
    pub pluggy_api_key: Option<String>,
    /// Accounts to import (ids from the Pluggy dashboard — stable).
    pub pluggy_accounts: Vec<PluggyAccountConf>,
}

/// One configured Pluggy account (from `PLUGGY_ACCOUNTS`, a JSON array).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluggyAccountConf {
    /// Pluggy account id (from the dashboard — never changes).
    pub id: String,
    /// Bank slug: 'nubank' | 'caixa' | 'c6'.
    pub bank: String,
    /// 'BANK' | 'CREDIT' — needed for sign conventions.
    pub kind: String,
    /// Display name.
    pub name: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);
        let session_secret = std::env::var("SESSION_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "deepsave-dev-secret-change-me-at-least-32-bytes".to_string());
        let password_hash = std::env::var("APP_PASSWORD_HASH")
            .ok()
            .filter(|s| !s.is_empty());
        let password_plain = std::env::var("APP_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty());
        let cookie_secure = std::env::var("COOKIE_SECURE")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let static_dir = std::env::var("STATIC_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "./frontend/dist".to_string());
        let storage_dir = std::env::var("STORAGE_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "./storage".to_string());

        let deepseek_api_key = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let deepseek_base_url = std::env::var("DEEPSEEK_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://api.deepseek.com".to_string());
        let deepseek_model = std::env::var("DEEPSEEK_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "deepseek-v4-flash".to_string());
        let deepseek_vision_model = std::env::var("DEEPSEEK_VISION_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "deepseek-v4-flash-vision-exp".to_string());
        let deepseek_pro_model = std::env::var("DEEPSEEK_PRO_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "deepseek-v4-pro".to_string());
        let deepseek_input_price_per_m = env_f64("DEEPSEEK_INPUT_PRICE_PER_M", 0.27);
        let deepseek_cache_hit_price_per_m = env_f64("DEEPSEEK_CACHE_HIT_PRICE_PER_M", 0.07);
        let deepseek_output_price_per_m = env_f64("DEEPSEEK_OUTPUT_PRICE_PER_M", 1.10);
        let coverage_months = std::env::var("COVERAGE_MONTHS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12);

        let pluggy_client_id = std::env::var("PLUGGY_CLIENT_ID")
            .ok()
            .filter(|s| !s.is_empty());
        let pluggy_client_secret = std::env::var("PLUGGY_CLIENT_SECRET")
            .ok()
            .filter(|s| !s.is_empty());
        let pluggy_api_key = std::env::var("PLUGGY_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let pluggy_accounts = std::env::var("PLUGGY_ACCOUNTS")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|raw| {
                serde_json::from_str::<Vec<PluggyAccountConf>>(&raw)
                    .context("PLUGGY_ACCOUNTS is not a valid JSON array")
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            database_url,
            port,
            session_secret,
            password_hash,
            password_plain,
            cookie_secure,
            static_dir,
            storage_dir,
            deepseek_api_key,
            deepseek_base_url,
            deepseek_model,
            deepseek_vision_model,
            deepseek_pro_model,
            deepseek_input_price_per_m,
            deepseek_cache_hit_price_per_m,
            deepseek_output_price_per_m,
            coverage_months,
            pluggy_client_id,
            pluggy_client_secret,
            pluggy_api_key,
            pluggy_accounts,
        })
    }
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
