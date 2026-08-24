# DeepSave — Completed Work (archive)

> Moved here from PLAN.md. Everything in this file is **implemented and verified**.
> For ongoing engineering conventions see `AGENTS.md`. For remaining work see `PLAN.md`.

## What was built

Single-user, self-hosted personal finance app: upload bank/card statements, receipts, and
payment slips; DeepSeek extracts, categorizes, and links items into a monthly spending tree.

- **Backend**: Rust (Axum) + PostgreSQL 16 (SQLx) + tokio
- **Frontend**: React 18 + Vite + TypeScript + Tailwind v4 + TanStack Query + Recharts
- **AI**: DeepSeek (`deepseek-v4-flash` text, `deepseek-v4-flash-vision-exp` vision, `deepseek-v4-pro` reserved)
- **Deploy**: multi-stage Dockerfile (backend + frontend + tesseract), docker-compose, CI

## Completed milestones

### M0 — Scaffolding
Repo layout, Axum hello-world, React/Vite/Tailwind skeleton, docker-compose, migrations, CI.

### M1 — Auth + core CRUD
Password-only login (argon2 + signed cookie session via tower-cookies), categories CRUD,
manual items CRUD with tree (`parent_id`), month list.

### M2 — Upload + extraction
Multipart upload with SHA256 dedupe, storage dir, document status lifecycle
(`pending → processing → needs_review | processed | failed`), background worker queue.
CSV parsers for **Nubank card / Nubank account / C6 invoice** (auto-detected; kind detection:
expense/income/transfer_out/transfer_in/refund; installment parsing).
PDF text via `pdf-extract`; image OCR via tesseract CLI with preprocessing (upscale + Otsu binarize).

### M3 — DeepSeek integration
`AiClient` (chat JSON mode + vision), retry/validation loop, `ai_calls` token/cost accounting
(cache hit/miss). Prompt caching via stable system prefix (instructions + category list + memory).
AI extraction → items as `pending_review`; review workflow (confirm / reject / edit).

### M4 — Linking + memory + transfers
Receipt→statement linking (document-level: receipt total → one statement item), match review UI.
`merchant_memory` upsert on confirm/edit + injection into prompts (gated `confirm_count ≥ 2`).
Accounts CRUD; transfer pairing (`transfer_out ↔ transfer_in` by amount + date).

### M5 — Dashboard & charts
`/api/dashboard` (totals, by_category, top_merchants, pending) + `/api/dashboard/trend`.
Dashboard page: summary cards, category pie, 12-month line, top merchants.

### M6 — Polish
Item search/filter (`search`, `category_id`, `kind`), item-level duplicate detection on ingest,
request logging (TraceLayer), 5 unit tests.

### M7 — Recurring items
`recurring_rules` CRUD + `/recurring` page (suggestions, rules, upcoming). Detection heuristics
(same merchant + similar amount + regular interval, ≥2 repeats; installments excluded).
Rules are **analytics-only** — they never create items. Dashboard "Recorrentes" count.

### M8 — Sources & coverage
`/coverage` page + dashboard "missing sources" alert. `sources` table (6 fundamental sources),
`documents.source_id` auto-detected from content, `COVERAGE_MONTHS` (default 12), startup backfill.

### M9 — New parsers + parser refactor
Moved parsers under `services/parsers/` (shared `ParsedItem`). Added **C6 bank statement CSV**
and **Caixa credit-card fatura PDF** parsers; `investment` + `card_payment` kinds; images→receipt.

### M10 — Tag management
`/tags` page: usage counts per tag, rename (cascades to `items`, `recurring_rules` and
`merchant_memory`; renaming into an existing tag merges), merge, delete.
Backend: `GET /api/tags/usage`, `PATCH /api/tags/rename`, `POST /api/tags/merge`,
`DELETE /api/tags/{tag}` (all tags normalized: lowercase + strip accents).

## Established decisions (as implemented)

- PostgreSQL (SQLite rejected); signed-cookie auth; single `APP_PASSWORD` / `APP_PASSWORD_HASH`.
- Amounts in cents: **negative = expense, positive = income**.
- `card_payment` kind for credit-card bill payments — **excluded from spend** (no double-count).
- `internal` kind for transfers between the user's own accounts — **excluded from spend/income**
  (set manually via bulk edit/item form or by the AI; parsers can't reliably detect self-transfers
  without the user's identity).
- C6 installments are dated by the **fatura billing month** (parsed from `Fatura_YYYY-MM-dd.csv`); "Única" items keep "Data de Compra".
- Images always classify as **`receipt`** (never card/bank statement).
- Linking is **document-level** and re-runnable (`POST /api/matches/suggest`).
- CSV items are created **`confirmed`**; AI-extracted items are **`pending_review`**.
- Transfer legs are excluded from spend totals and paired in a dedicated view.
- Merchant memory is advisory (few-shot) and only injected after 2 confirmations.

## Resolved decisions (from planning)

1. **Recurring timing**: next occurrence auto-created a few days before `next_due_on` (default 3 days).
2. **Recurring detection**: suggest after 2 repeats, require user confirmation.
3. **Caixa**: PDF only (no CSV export).
