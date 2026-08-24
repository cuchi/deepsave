# AGENTS.md

Guidance for agents working on this repo. The design source of truth is [`PLAN.md`](./PLAN.md) — read it before making architectural changes.

## What this is

DeepSave: single-user, self-hosted personal finance app. Upload statements/receipts, DeepSeek extracts/categorizes/link items into a monthly spending tree.

- Backend: Rust (Axum + SQLx 0.9 + PostgreSQL 16)
- Frontend: React 18 + Vite + TypeScript + Tailwind v4 + TanStack Query
- AI: DeepSeek (models: `deepseek-v4-flash`, `deepseek-v4-pro`, `deepseek-v4-flash-vision-exp`)

## Layout

- `backend/` — Axum server; migrations in `backend/migrations/` (auto-run at startup)
- `frontend/` — Vite SPA; dev server proxies `/api` → `localhost:8080`
- `examples/` — real sample statements/receipts (do not commit more PII without asking)
- `scripts/seed.sh` — resets + seeds fake data (run from host, uses `docker exec`)
- `Dockerfile` — multi-stage: builds frontend + backend, serves both from one image

## Commands

```bash
docker compose up --build          # full app at :8080
docker compose watch               # dev: rebuilds & recreates the app container on code changes (file-watch)
./scripts/seed.sh                  # seed fake data
cd backend && cargo run            # dev backend (needs postgres up)
cd frontend && npm run dev         # dev frontend at :5173
cd backend && cargo test           # backend tests (needs DATABASE_URL)
cd frontend && npm run build       # type-check + build frontend
```

## Tests

- All backend tests live in `backend/tests/` (unit tests in `tests/parsers.rs`, full-flow
  integration tests in `tests/ingestion.rs`).
- Fixtures are **fake/anonymized** under `backend/tests/fixtures/` — never use `examples/`
  in tests (it's gitignored and contains real data).
- Integration tests use `#[sqlx::test]` (needs a Postgres reachable via `DATABASE_URL`,
  e.g. the docker-compose Postgres) and `wiremock` to fake the DeepSeek API.

## Conventions & gotchas

- **Amounts**: integer cents, **negative = expense, positive = income** (`items.amount_cents`).
- **Tree**: `items.parent_id` self-reference; receipt lines are children of statement items.
- **sqlx 0.9 specifics**:
  - Non-literal SQL must be wrapped in `sqlx::AssertSqlSafe(...)`; prefer `&'static str` consts.
  - `Uuid` mapping requires the sqlx `uuid` feature (already enabled).
  - `SELECT 1` returns `int4` — use `SELECT 1::bigint` for `i64`.
- **argon2** needs its `std` feature (already enabled) for `?` → `anyhow::Error` conversion.
- **`SESSION_SECRET` must be ≥ 32 bytes** (`Key::derive_from` panics otherwise; guarded at startup).
- **Migrations**: `sqlx::migrate!("./migrations")` embeds files at compile time — the `migrations/` dir must exist when building (including in the Docker build stage).
- **Static serving**: backend serves the built frontend from `STATIC_DIR` (SPA fallback to `index.html`); keep `/api/*` routes above the fallback.
- **Auth**: single password (argon2), signed cookie `deepsave_session`; protected routes use `auth::require_auth` middleware. Dev default password: `deepsave`.
- **Errors**: return `AppError` (in `backend/src/error.rs`) from handlers; it maps to JSON `{ "error": ... }`.
- **JSON field names are snake_case** (match the Rust structs / DB columns).
- **Documents are processed asynchronously** by a background worker (`backend/src/services/queue.rs`); `documents.status` goes `pending → processing → processed | failed`. Uploaded files live under `STORAGE_DIR` (container: `/app/storage`).
- **Parsers** live in `backend/src/services/parsers/` — `csv.rs` (Nubank card/account, C6 card/bank) and `caixa_card.rs` (Caixa credit-card fatura PDF). Add new bank formats there. They produce `ParsedItem`s that are inserted as `confirmed`.
- **OCR/PDF** text extraction is in `backend/src/services/extract.rs` (tesseract CLI + pdf-extract); AI structuring is the fallback for non-structured PDFs.
- **Linking / memory** live in `backend/src/services/{linking,memory}.rs`. Receipt→statement match suggestions are created on receipt ingestion; confirm them via `/api/matches/{id}/accept`. Merchant memory is upserted on item confirm/edit and injected into AI prompts (gated at `confirm_count >= 2`).
- **Categories are intrinsic to a merchant; tags are situational.** Memory auto-applies only the *category* (`apply-memory` / `apply-all`); tags are per-item and never bulk-applied.
- **Recurring items** live in `backend/src/services/recurring.rs` + `routes/recurring.rs`. Detection is suggestion-only (user confirms). Rules are **analytics-only — they never auto-create items** (avoids double-counting with statement items); `upcoming` is a pure forecast.

## Before committing

- `cargo build` (backend) and `npm run build` (frontend) must pass.
- Add a SQL migration for any schema change; don't edit already-applied migrations.
- Keep AI prompts pt-BR and the DeepSeek call accounting in the `ai_calls` table.
