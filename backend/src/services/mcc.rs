//! MCC → category rule: card purchases carry a Merchant Category Code (Pluggy's
//! `creditCardMetadata.payeeMCC`) that identifies the kind of business. This
//! gives us a deterministic, zero-AI-cost category for a large share of card
//! items. The mapping targets our seeded category names (Supermercado,
//! Transporte, Restaurantes, Saúde, Moradia, Lazer, Assinaturas, Outros).

use anyhow::Result;
use sqlx::PgPool;
use tracing::info;

/// Canonical category name for an MCC, or `None` when unknown.
pub fn category_for_mcc(mcc: i32) -> Option<&'static str> {
    let map: &[(i32, &str)] = &[
        // Supermercado
        (5411, "Supermercado"),
        (5412, "Supermercado"),
        (5422, "Supermercado"),
        (5499, "Supermercado"),
        (5441, "Supermercado"),
        (5451, "Supermercado"),
        // Restaurantes
        (5811, "Restaurantes"),
        (5812, "Restaurantes"),
        (5813, "Restaurantes"),
        (5814, "Restaurantes"),
        (5462, "Restaurantes"),
        // Transporte
        (4111, "Transporte"),
        (4112, "Transporte"),
        (4121, "Transporte"),
        (4784, "Transporte"),
        (5541, "Transporte"),
        (5542, "Transporte"),
        (5544, "Transporte"),
        (5546, "Transporte"),
        (5547, "Transporte"),
        (7512, "Transporte"),
        (7513, "Transporte"),
        (7523, "Transporte"),
        // Saúde
        (5912, "Saúde"),
        (5975, "Saúde"),
        (5976, "Saúde"),
        (5977, "Saúde"),
        (8011, "Saúde"),
        (8021, "Saúde"),
        (8031, "Saúde"),
        (8042, "Saúde"),
        (8043, "Saúde"),
        (8049, "Saúde"),
        (8062, "Saúde"),
        (8090, "Saúde"),
        (6300, "Saúde"), // insurance
        // Moradia (utilities / housing)
        (4814, "Moradia"),
        (4816, "Moradia"),
        (4900, "Moradia"),
        (5211, "Moradia"), // home supply stores
        (5712, "Moradia"), // furniture
        (5713, "Moradia"),
        (5714, "Moradia"),
        (5722, "Moradia"),
        (6513, "Moradia"), // real estate
        // Lazer (travel / entertainment)
        (7011, "Lazer"),
        (7012, "Lazer"),
        (7832, "Lazer"),
        (7833, "Lazer"),
        (7922, "Lazer"),
        (7933, "Lazer"),
        (7941, "Lazer"),
        (7991, "Lazer"),
        (7992, "Lazer"),
        (7993, "Lazer"),
        (7994, "Lazer"),
        (7995, "Lazer"),
        (7996, "Lazer"),
        (7997, "Lazer"),
        (7998, "Lazer"),
        (7999, "Lazer"),
        (4722, "Lazer"), // travel agencies
        // Assinaturas (digital goods / streaming / subscriptions)
        (5815, "Assinaturas"), // digital goods
        (5816, "Assinaturas"), // digital games
        (5817, "Assinaturas"), // streaming
        (5818, "Assinaturas"), // streaming
        (5968, "Assinaturas"), // continuity/subscription (Amazon Prime etc.)
        (5969, "Assinaturas"),
        (4812, "Assinaturas"), // telecom
        (4899, "Assinaturas"), // cable/satellite
        // Outros — retail / general merchandise / services
        (5310, "Outros"),
        (5311, "Outros"),
        (5331, "Outros"),
        (5399, "Outros"),
        (5611, "Outros"),
        (5621, "Outros"),
        (5631, "Outros"),
        (5641, "Outros"),
        (5651, "Outros"),
        (5661, "Outros"),
        (5691, "Outros"),
        (5192, "Outros"), // bookstore
        (5940, "Outros"), // bicycles
        (5941, "Outros"), // sporting goods
        (5942, "Outros"), // bookstore
        (5943, "Outros"), // stationery
        (5944, "Outros"), // jewelry
        (5945, "Outros"), // hobby
        (5946, "Outros"), // camera
        (5947, "Outros"), // gifts
        (5948, "Outros"), // luggage
        (5949, "Outros"), // sewing
        (5732, "Outros"), // electronics
        (5733, "Outros"),
        (5734, "Outros"), // computer software
        (5992, "Outros"), // florists
        (5993, "Outros"), // cigar stores
        (5995, "Outros"), // pet stores
        (5996, "Outros"), // pool/patio
        (5997, "Outros"), // eyeglasses
        (5999, "Outros"), // misc retail
        (7299, "Outros"), // misc services
        (7399, "Outros"), // misc business services
        (8999, "Outros"), // professional services
        (6011, "Outros"), // financial
        (6051, "Outros"), // crypto / non-financial institutions
    ];
    map.iter().find(|(m, _)| *m == mcc).map(|(_, c)| *c)
}

/// Apply the MCC rule: for every item with an MCC but no category yet, set the
/// category from the mapping (matched accent/case-insensitively to an existing
/// category; unknown categories are skipped). Idempotent — only touches
/// uncategorized items. Returns how many were categorized.
pub async fn apply_mcc_categories(pool: &PgPool) -> Result<usize> {
    let cats: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT id, name FROM categories WHERE is_active")
        .fetch_all(pool)
        .await?;

    let rows: Vec<(uuid::Uuid, i32)> = sqlx::query_as(
        "SELECT id, mcc FROM items WHERE mcc IS NOT NULL AND category_id IS NULL AND kind = 'expense'",
    )
    .fetch_all(pool)
    .await?;

    let mut updated = 0usize;
    for (id, mcc) in rows {
        let Some(name) = category_for_mcc(mcc) else {
            continue;
        };
        let Some(cat_id) = cats.iter().find(|(_, n)| {
            crate::services::tags::strip_accents(n).to_lowercase()
                == crate::services::tags::strip_accents(name).to_lowercase()
        }) else {
            // Category name from the mapping not found (e.g. deleted) — skip.
            continue;
        };
        let res = sqlx::query(
            "UPDATE items SET category_id = $1, updated_at = now() WHERE id = $2 AND category_id IS NULL",
        )
        .bind(cat_id.0)
        .bind(id)
        .execute(pool)
        .await?;
        updated += res.rows_affected() as usize;
    }
    if updated > 0 {
        info!("mcc rule: categorized {updated} item(s)");
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_mccs() {
        assert_eq!(category_for_mcc(5411), Some("Supermercado"));
        assert_eq!(category_for_mcc(5812), Some("Restaurantes"));
        assert_eq!(category_for_mcc(5542), Some("Transporte"));
        assert_eq!(category_for_mcc(5912), Some("Saúde"));
        assert_eq!(category_for_mcc(7011), Some("Lazer"));
        assert_eq!(category_for_mcc(5968), Some("Assinaturas"));
        assert_eq!(category_for_mcc(5818), Some("Assinaturas"));
        assert_eq!(category_for_mcc(7399), Some("Outros")); // services
        assert_eq!(category_for_mcc(9999), None); // unknown → AI decides
    }

}
