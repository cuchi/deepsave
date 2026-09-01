//! Scheduled background jobs. Currently: the daily Pluggy sync (last 7 days).

use std::time::Duration;
use tracing::{error, info, warn};

use crate::config::PluggyAccountConf;
use crate::services::{mcc, pluggy};
use crate::PluggyHandle;

/// Daily job: pull the latest 7 days from every configured Pluggy account,
/// then run the deterministic post-import steps (MCC rule, installment series,
/// refund linking). First run shortly after startup (covers a missed day),
/// then every 24h. Failures are logged and the loop keeps going — a stale
/// PLUGGY_API_KEY (2h JWT) or a transient network error must not kill it.
pub async fn run_daily_pluggy_sync(
    pool: sqlx::PgPool,
    pluggy: PluggyHandle,
    accounts: Vec<PluggyAccountConf>,
    enabled: bool,
) {
    if !enabled {
        info!("daily pluggy sync disabled (DAILY_PLUGGY_SYNC=false)");
        return;
    }
    tokio::time::sleep(Duration::from_secs(45)).await;
    loop {
        if let Err(e) = sync_once(&pool, &pluggy, &accounts).await {
            error!("daily pluggy sync failed: {e:#}");
        }
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    }
}

async fn sync_once(
    pool: &sqlx::PgPool,
    pluggy: &PluggyHandle,
    accounts: &[PluggyAccountConf],
) -> anyhow::Result<()> {
    let Some(client) = pluggy.client() else {
        warn!("daily pluggy sync: Pluggy not configured — skipping");
        return Ok(());
    };
    // Pick up any .env changes without a restart.
    pluggy::seed_configured_accounts(pool, accounts).await?;
    let today = chrono::Utc::now().date_naive();
    let from = today - chrono::Duration::days(7);
    let results = pluggy::sync_all_accounts(pool, client, Some(from), Some(today)).await?;
    let mcc_categorized = mcc::apply_mcc_categories(pool).await?;
    let series = pluggy::assign_installment_series(pool).await?;
    let linked_refunds = pluggy::link_refunds(pool).await?;
    let total: usize = results.iter().map(|r| r.new).sum();
    info!(
        "daily pluggy sync (last 7 days): new_items={total} mcc_categorized={mcc_categorized} \
         series={series} refunds_linked={linked_refunds} accounts={}",
        results.len()
    );
    Ok(())
}
