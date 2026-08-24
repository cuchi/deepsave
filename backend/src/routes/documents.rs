use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::header;
use axum::response::Response;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;
use crate::error::AppError;
use crate::models::{DocumentDetail, DocumentRow, DocumentSummary, Item};

const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const ALLOWED_KINDS: &[&str] = &[
    "card_statement",
    "bank_statement",
    "receipt",
    "payment_slip",
];

const DOC_COLS: &str = "id, kind, account_id, source_id, filename, content_type, sha256, file_path, \
     status, error_message, ocr_text, uploaded_at, processed_at";

const DOC_SUMMARY_COLS: &str = "id, kind, filename, content_type, status, error_message, \
     uploaded_at, processed_at, source_id";

pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<DocumentSummary>, AppError> {
    let mut kind: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut data: Vec<u8> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("multipart error: {e}")))?
    {
        match field.name().unwrap_or("") {
            "kind" => {
                kind = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::bad_request(format!("invalid kind field: {e}")))?,
                );
            }
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                content_type = field.content_type().map(|s| s.to_string());
                data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::bad_request(format!("failed to read file: {e}")))?
                    .to_vec();
            }
            _ => {}
        }
    }

    let filename = filename
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("missing file"))?;
    let content_type = content_type.unwrap_or_default();

    if data.is_empty() {
        return Err(AppError::bad_request("empty file"));
    }
    if data.len() > MAX_UPLOAD_BYTES {
        return Err(AppError::bad_request("file too large (max 20MB)"));
    }

    let kind = detect_kind(kind, &filename, &content_type, &data);
    if !ALLOWED_KINDS.contains(&kind.as_str()) {
        return Err(AppError::bad_request("invalid document kind"));
    }

    let source_id =
        crate::services::sources::detect_source(&state.pool, &filename, &content_type, &data)
            .await?;

    let sha = hex::encode(Sha256::digest(&data));

    let existing = sqlx::query_scalar::<_, Uuid>("SELECT id FROM documents WHERE sha256 = $1")
        .bind(&sha)
        .fetch_optional(&state.pool)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict("document already uploaded".into()));
    }

    let safe_name = sanitize_filename(&filename);
    let abs_path = state
        .storage_dir
        .join(format!("{}_{safe_name}", Uuid::new_v4()));

    tokio::fs::write(&abs_path, &data)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let doc = sqlx::query_as::<_, DocumentSummary>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO documents (kind, filename, content_type, sha256, file_path, status, source_id)
         VALUES ($1, $2, $3, $4, $5, 'pending', $6)
         RETURNING {DOC_SUMMARY_COLS}, 0::bigint AS item_count"
    )))
    .bind(&kind)
    .bind(&filename)
    .bind(&content_type)
    .bind(&sha)
    .bind(abs_path.to_string_lossy().to_string())
    .bind(source_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(doc))
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<DocumentSummary>>, AppError> {
    let docs = sqlx::query_as::<_, DocumentSummary>(
        "SELECT d.id, d.kind, d.filename, d.content_type, d.status, d.error_message,
                d.uploaded_at, d.processed_at, d.source_id,
                (SELECT count(*) FROM items i WHERE i.document_id = d.id) AS item_count
         FROM documents d
         ORDER BY d.uploaded_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(docs))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DocumentDetail>, AppError> {
    let doc = sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {DOC_COLS} FROM documents WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("document not found"))?;

    let items = sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM items WHERE document_id = $1 ORDER BY occurred_on",
        crate::routes::items::ITEM_COLS
    )))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(DocumentDetail {
        id: doc.id,
        kind: doc.kind,
        filename: doc.filename,
        content_type: doc.content_type,
        status: doc.status,
        error_message: doc.error_message,
        uploaded_at: doc.uploaded_at,
        processed_at: doc.processed_at,
        ocr_text: doc.ocr_text,
        items,
        source_id: doc.source_id,
    }))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let doc = sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {DOC_COLS} FROM documents WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("document not found"))?;

    sqlx::query("DELETE FROM items WHERE document_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    let _ = tokio::fs::remove_file(&doc.file_path).await;

    Ok(Json(json!({ "ok": true })))
}

/// Serve the raw uploaded file (used to open a receipt photo in a new tab).
pub async fn file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let doc = sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {DOC_COLS} FROM documents WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("document not found"))?;

    let bytes = tokio::fs::read(&doc.file_path)
        .await
        .map_err(|_| AppError::not_found("file not found"))?;

    let content_type = if doc.content_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        doc.content_type.clone()
    };

    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{}\"", doc.filename),
        )
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.into()))
}

/// Re-run ingestion for a document: delete its items and re-enqueue it.
pub async fn reprocess(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents WHERE id = $1)")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(AppError::not_found("document not found"));
    }

    sqlx::query("DELETE FROM items WHERE document_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    sqlx::query(
        "UPDATE documents SET status = 'pending', error_message = NULL, processed_at = NULL, ocr_text = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({ "ok": true })))
}

fn detect_kind(kind: Option<String>, filename: &str, content_type: &str, data: &[u8]) -> String {
    let lower = filename.to_lowercase();
    let is_image = content_type.starts_with("image/")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png");
    // Images are always receipts (or receipt-like slips); never a card/bank statement.
    if is_image {
        return "receipt".into();
    }
    if let Some(k) = kind.filter(|s| !s.is_empty()) {
        return k;
    }
    if lower.ends_with(".csv") {
        let head = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_lowercase();
        if head.contains("data de compra") {
            return "card_statement".into();
        }
        if head.contains("data lançamento")
            || head.contains("extrato de conta corrente")
            || head.contains("identificador")
        {
            return "bank_statement".into();
        }
        return "card_statement".into();
    }
    "card_statement".into()
}

fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let sanitized: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        sanitized
    }
}
