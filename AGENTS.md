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
- **Pluggy enrichment metadata**: `items.pluggy_category`, `items.mcc`, `items.operation_type`, `items.payment_method` are stored at import (enrichment-only update on conflict, so a forced resync backfills them) and injected into the AI prompts (`pc`/`mcc`/`op`/`pay`). The **MCC → category rule** (`services/mcc.rs`, runs on every sync) deterministically categorizes uncategorized card items (5411→Supermercado, 5812→Restaurantes, 5542→Transporte, 5912→Saúde, 5817/5818/5968→Assinaturas, retail→Outros…).
- **AI proposals** live in `backend/src/services/ai_tags.rs` + `routes/ai_tags.rs` (batches carry `kind`: 'tags' | 'categorize' | 'full' — the **`full`** kind suggests category + tags in one call; `ai_tag_suggestions.suggested_category` holds category proposals, "nova: X" creates the category). Suggestions are reviewed **one-by-one in a modal** (SuggestionReviewModal: per-field apply checkboxes for category/tags, editable category dropdown incl. "sem categoria" (clears, sentinel `__none__`) and "nova…", ←/→ navigation) or in bulk from the amber banner (Aplicar/Ignorar todas, "Só itens com sugestão" filter). **Tags that are also category names are never suggested** (filtered from the AI vocabulary and dropped post-response via `category_norms`). Tag descriptions (F0, `tags` registry table + `tags::tag_descriptions`) are injected into every prompt.
- **Change log** (`services/change_log.rs`, migration 0021): append-only record of every category/tag change the user makes (`change_log`, source = item_edit|bulk|memory_apply|ai_apply), keyed by normalized merchant-or-description identity. Logged on item edit, bulk edit, AI apply (single/all) and memory apply; the Memória page has a "Histórico" tab (`GET /api/change-log`). Every AI prompt receives `hist` (last 5 changes per merchant) so the model follows the user's latest decisions.
- **No merchant-memory table** — `merchant_memory` was decommissioned (migration 0023; its rows were migrated into `change_log` as `source='legacy'`). The AI learns **only** from the change log (`hist`), tagged-item examples (`ex`) and Pluggy metadata (`mcc`/`pluggy_category`/`operation_type`).
- **Categories are intrinsic to a merchant; tags are situational.** The user's decisions live in the change log; tag rename/merge/delete cascade to items, recurring rules and the tags registry.
- **Recurring items** live in `backend/src/services/recurring.rs` + `routes/recurring.rs`. Rules are **analytics-only — they never auto-create items** (avoids double-counting with statement items); the forecast/`upcoming` feed is derived from rules + purchase series. Matching is **alias-based** (normalized exact equality against `recurring_aliases`; `matches_alias` tolerates a trailing amount, so one alias covers "PREST HAB 1847,32" / "PREST HAB 1832,10"); isolated-case entries are one-shot manual references. Manual linking (`POST /items/{id}/link-recurring`) sets `items.recurring_id` + `linked_manually` and wins over automation. Rule tags are **derived** from linked items (union + `tags_conflict` flag); `next_due_on` is never in the past (`advance_next_due`).
- **Pluggy** (open-banking aggregation) lives in `backend/src/services/pluggy.rs` + `routes/pluggy.rs`, UI merged into System (the old `/pluggy` route redirects there). **Config-driven, no items API**: account ids are read once from the Pluggy dashboard (they are stable) and listed in `PLUGGY_ACCOUNTS` (JSON array: `id`, `bank` `nubank|caixa|c6`, `kind` `BANK|CREDIT`, `name`); seeded into `pluggy_accounts` at startup and re-seeded on every sync. Auth is **API-key first**: `PLUGGY_API_KEY` (single-user setups) is sent directly as `X-API-KEY` and never refreshed; `PLUGGY_CLIENT_ID`/`PLUGGY_CLIENT_SECRET` are the fallback (`/auth` JWT flow, cached 90 min, auto-refreshed on 401). `POST /pluggy/sync` pulls `/v2/transactions` (cursor-paginated; the old `/transactions` is 410-deprecated) per account and imports them as `confirmed` items (`source = 'pluggy'`), idempotent via `items.external_id` (partial unique index). **Incremental by default**: `sync_all_accounts` fetches only transactions newer than the account's last imported date (minus a 3-day overlap for late-posted charges); pass `?from=&to=` (YYYY-MM-DD) to force a full re-pull of a custom period (the Pluggy UI has date inputs for this). Only new transactions are inserted (`ON CONFLICT DO NOTHING`) — existing rows are never touched. `link_refunds` (runs on every sync) links each refund to the expense it reverses. `assign_installment_series` (also on every sync) groups installment items into `purchase_series` keyed by (account, normalized merchant tokens, installment count) so the **forecast/upcoming** can project future parcels (Pluggy items used to have no `series_id` and were invisible to the forecast). **Signs**: bank DEBIT → negative expense, CREDIT → positive income; card charges (positive DEBIT) → negative expense with `installment/installment_count` from `creditCardMetadata`; card-side payments/refunds (CREDIT on card accounts) are skipped; bank-side `Credit card payment` → `internal` (already on the card side). `pluggy_accounts` maps 1:1 to `accounts` rows. Per-item `bank` (legacy via `documents→sources`, Pluggy via `pluggy_accounts`) is computed in the item queries (`ITEM_COLS` / the list query); `GET /api/banks` feeds the filter dropdowns. **Refund netting**: `items.refunded_item_id` links a refund to the expense it reverses (`link_refunds` runs on every sync — exact |amount| + merchant tokens first, partial second; kind-agnostic, greedy, nearest-date tie-break). Graphs (`dashboard`, `trend`, `daily`, `categories`, `tags`) treat a linked refund as a negative expense bucketed by its **charge's** month/category/merchant (`NET_AMOUNT`/`BUCKET_*` fragments + the `rc` self-join), so a refunded expense nets to zero and nothing inflates; refunds linked to `internal` charges and unlinked refunds are left alone.

## Before committing

- `cargo build` (backend) and `npm run build` (frontend) must pass.
- Add a SQL migration for any schema change; don't edit already-applied migrations.
- Keep AI prompts pt-BR and the DeepSeek call accounting in the `ai_calls` table.
