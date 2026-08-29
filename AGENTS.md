# AGENTS.md

Guidance for agents working on this repo. The design source of truth is [`PLAN.md`](./PLAN.md) — read it before making architectural changes.

## What this is

DeepSave: single-user, self-hosted personal finance app. Transactions are imported from the banks via **Pluggy** (open-banking API) and categorized into a monthly spending tree. The old document-upload import (statements/faturas/receipts) has been **decommissioned** — the tables and rows remain as legacy data, but nothing imports through them anymore.

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

- All backend tests live in `backend/tests/`. Unit tests for the Pluggy mapping/dedupe
  matcher are in `backend/src/services/pluggy.rs` (`#[cfg(test)]`); the full import
  pipeline is exercised in `tests/pluggy.rs`.
- Integration tests use `#[sqlx::test]` (needs a Postgres reachable via `DATABASE_URL`,
  e.g. the docker-compose Postgres) and `wiremock` to fake the Pluggy/DeepSeek APIs.

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
- **Memory** lives in `backend/src/services/memory.rs`. Merchant memory (category + tags) is upserted on item confirm, on AI-tag apply, and on single-item edit (opt-out via `ItemInput.update_memory`, defaults on — the edit form has a "Salvar na memória" checkbox). Bulk applies go through **preview first** (`POST /memory/preview` lists the items that would change; `POST /memory/apply` touches only the selected ids).
- **Categories are intrinsic to a merchant; tags are situational.** Memory now records both:
  `merchant_memory` carries `category_id` (latest wins) **and** `tags` (accumulate/union over
  confirmations). Applying memory (`apply-memory` per item, `POST /memory/preview` + `/memory/apply`
  for bulk) sets the category (replace) **and adds** the remembered tags (never removes item tags).
  Tag rename/merge/delete cascade to items, recurring rules and merchant memory.
- **Recurring items** live in `backend/src/services/recurring.rs` + `routes/recurring.rs`. Detection is suggestion-only (user confirms). Rules are **analytics-only — they never auto-create items** (avoids double-counting with statement items); `upcoming` is a pure forecast. Alias matching tolerates a trailing amount in the item name (`matches_alias`), so one alias covers varying payments like "PREST HAB 1847,32" / "PREST HAB 1832,10".
- **Pluggy** (open-banking aggregation) lives in `backend/src/services/pluggy.rs` + `routes/pluggy.rs`, UI at `/pluggy`. **Config-driven, no items API**: account ids are read once from the Pluggy dashboard (they are stable) and listed in `PLUGGY_ACCOUNTS` (JSON array: `id`, `bank` `nubank|caixa|c6`, `kind` `BANK|CREDIT`, `name`); seeded into `pluggy_accounts` at startup and re-seeded on every sync. Auth is **API-key first**: `PLUGGY_API_KEY` (single-user setups) is sent directly as `X-API-KEY` and never refreshed; `PLUGGY_CLIENT_ID`/`PLUGGY_CLIENT_SECRET` are the fallback (`/auth` JWT flow, cached 90 min, auto-refreshed on 401). `POST /pluggy/sync` pulls `/v2/transactions` (cursor-paginated; the old `/transactions` is 410-deprecated) per account and imports them as `confirmed` items (`source = 'pluggy'`), idempotent via `items.external_id` (partial unique index). **Incremental by default**: `sync_all_accounts` fetches only transactions newer than the account's last imported date (minus a 3-day overlap for late-posted charges); pass `?from=&to=` (YYYY-MM-DD) to force a full re-pull of a custom period (the Pluggy UI has date inputs for this). Only new transactions are inserted (`ON CONFLICT DO NOTHING`) — existing rows are never touched. `link_refunds` (runs on every sync) links each refund to the expense it reverses. **Signs**: bank DEBIT → negative expense, CREDIT → positive income; card charges (positive DEBIT) → negative expense with `installment/installment_count` from `creditCardMetadata`; card-side payments/refunds (CREDIT on card accounts) are skipped; bank-side `Credit card payment` → `internal` (already on the card side). `pluggy_accounts` maps 1:1 to `accounts` rows. Per-item `bank` (legacy via `documents→sources`, Pluggy via `pluggy_accounts`) is computed in the item queries (`ITEM_COLS` / the list query); `GET /api/banks` feeds the filter dropdowns. **Refund netting**: `items.refunded_item_id` links a refund to the expense it reverses (`link_refunds` runs on every sync — exact |amount| + merchant tokens first, partial second; kind-agnostic, greedy, nearest-date tie-break). Graphs (`dashboard`, `trend`, `daily`, `categories`, `tags`) treat a linked refund as a negative expense bucketed by its **charge's** month/category/merchant (`NET_AMOUNT`/`BUCKET_*` fragments + the `rc` self-join), so a refunded expense nets to zero and nothing inflates; refunds linked to `internal` charges and unlinked refunds are left alone.

## Before committing

- `cargo build` (backend) and `npm run build` (frontend) must pass.
- Add a SQL migration for any schema change; don't edit already-applied migrations.
- Keep AI prompts pt-BR and the DeepSeek call accounting in the `ai_calls` table.
