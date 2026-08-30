/// Normalize a set of tags: trim, lowercase, strip accents, dedupe, drop empties.
pub fn normalize(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        let t = strip_accents(tag.trim()).to_lowercase();
        if t.is_empty() {
            continue;
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

/// Strip common diacritics (Portuguese / Latin-1) from a string.
pub fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'Ç' => 'C',
            _ => c,
        })
        .collect()
}

/// Normalize a single tag, returning `None` if the result would be empty.
pub fn normalize_one(s: &str) -> Option<String> {
    let mut out = normalize(&[s.to_string()]);
    out.pop()
}

/// Usage counts per tag across all items.
pub async fn usage(pool: &sqlx::PgPool) -> Result<Vec<crate::models::TagUsage>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::TagUsage>(
        "SELECT tag, count(*) AS count
         FROM items CROSS JOIN LATERAL unnest(tags) AS tag
         GROUP BY tag
         ORDER BY count DESC, tag ASC",
    )
    .fetch_all(pool)
    .await
}

/// Result of a rename/merge/delete operation across the tables that carry tags.
/// (Recurring rules are not included: their tags are derived from linked items.)
#[derive(Debug, Clone, Copy, Default)]
pub struct TagRename {
    pub items_updated: u64,
}

/// Replace `from` with `to` everywhere tags are stored. If `to` already exists on a
/// row, `from` is dropped instead (dedupe) — i.e. renaming into an existing tag
/// behaves as a merge. Tags within an array are unique, so `array_replace` can
/// introduce at most one duplicate, handled by the `CASE`.
pub async fn rename(pool: &sqlx::PgPool, from: &str, to: &str) -> Result<TagRename, sqlx::Error> {
    let items_updated = sqlx::query(
        "UPDATE items
         SET tags = sub.new_tags, updated_at = now()
         FROM (
             SELECT id, CASE
                 WHEN $2 = ANY(tags) THEN array_remove(tags, $1)
                 ELSE array_replace(tags, $1, $2)
             END AS new_tags
             FROM items
             WHERE $1 = ANY(tags)
         ) sub
         WHERE items.id = sub.id",
    )
    .bind(from)
    .bind(to)
    .execute(pool)
    .await?
    .rows_affected();

    // Registry: when the target already has a description, the source merges into
    // it (target's description wins); otherwise carry the source's row over.
    sqlx::query("DELETE FROM tags WHERE name = $1 AND EXISTS (SELECT 1 FROM tags WHERE name = $2)")
        .bind(from)
        .bind(to)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE tags SET name = $2 WHERE name = $1")
        .bind(from)
        .bind(to)
        .execute(pool)
        .await?;

    Ok(TagRename { items_updated })
}

/// Drop a tag from every row that carries it.
pub async fn remove(pool: &sqlx::PgPool, tag: &str) -> Result<TagRename, sqlx::Error> {
    let items_updated = sqlx::query(
        "UPDATE items SET tags = array_remove(tags, $1), updated_at = now() WHERE $1 = ANY(tags)",
    )
    .bind(tag)
    .execute(pool)
    .await?
    .rows_affected();

    sqlx::query("DELETE FROM tags WHERE name = $1")
        .bind(tag)
        .execute(pool)
        .await?;

    Ok(TagRename {
        items_updated,
    })
}

/// Set (or create) the description of a tag in the registry.
pub async fn upsert_description(pool: &sqlx::PgPool, tag: &str, description: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tags (name, description) VALUES ($1, $2)
         ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description",
    )
    .bind(tag)
    .bind(description)
    .execute(pool)
    .await?;
    Ok(())
}

/// Registry descriptions for a set of tags (empty map when none are described).
/// Used to inject "tag X = <desc>" context into AI prompts.
pub async fn tag_descriptions(
    pool: &sqlx::PgPool,
    tags: &[String],
) -> Result<std::collections::HashMap<String, String>, sqlx::Error> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, description FROM tags WHERE description <> '' AND name = ANY($1)")
            .bind(tags)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}
