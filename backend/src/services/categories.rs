//! Category name resolution: accent/case-insensitive matching against the
//! active category table. Shared by the MCC rule, the Pluggy category import
//! and the AI categorization flow so they all agree on what "same category"
//! means.

use sqlx::PgPool;
use uuid::Uuid;

use crate::services::tags;

/// Fold a category name for comparison: trim + strip accents + lowercase.
/// Unlike [`tags::fold_key`], spaces and punctuation are preserved
/// (`Saúde (teste)` → `saude (teste)`), so parenthesized variants stay distinct.
pub fn norm(name: &str) -> String {
    tags::strip_accents(name.trim()).to_lowercase()
}

/// Active categories as `(id, name)`, ordered by name.
pub async fn load_active(pool: &PgPool) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as("SELECT id, name FROM categories WHERE is_active ORDER BY name")
        .fetch_all(pool)
        .await
}

/// Find an active category by exact normalized name.
pub fn find_by_name(cats: &[(Uuid, String)], name: &str) -> Option<Uuid> {
    let target = norm(name);
    if target.is_empty() {
        return None;
    }
    cats.iter()
        .find(|(_, n)| norm(n) == target)
        .map(|(id, _)| *id)
}

/// Find by exact name, else best-effort substring match in either direction
/// (e.g. Pluggy's `"food delivery"` vs our `"restaurantes"` are pre-mapped, but
/// loose matching still helps with unaliased taxonomy names).
pub fn find_by_name_loose(cats: &[(Uuid, String)], name: &str) -> Option<Uuid> {
    let target = norm(name);
    if target.is_empty() {
        return None;
    }
    find_by_name(cats, name).or_else(|| {
        cats.iter()
            .find(|(_, n)| {
                let nl = norm(n);
                !nl.is_empty() && (target.contains(&nl) || nl.contains(&target))
            })
            .map(|(id, _)| *id)
    })
}
