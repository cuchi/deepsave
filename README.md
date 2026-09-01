# DeepSave

**AI-augmented personal finance manager — single-user, self-hosted, pt-BR interface.**

DeepSave imports your transactions from the banks via [Pluggy](https://pluggy.ai)
(open-banking API), categorizes them with DeepSeek into a monthly spending tree, and
learns your category/tag decisions over time — no spreadsheets, no manual
categorization of every purchase.

- **Backend**: Rust (Axum 0.8, SQLx 0.9, tokio) + PostgreSQL 16
- **Frontend**: React 18 + Vite + TypeScript + Tailwind v4 + TanStack Query + ECharts
- **AI**: DeepSeek (`deepseek-v4-flash` default; vision/pro model slots in `.env.example`)
- **Deploy**: single multi-stage Docker image (backend + built SPA), docker-compose, GitHub Actions CI

> The old document-upload import (statements, faturas, receipts) has been
> **decommissioned** in favor of Pluggy. Legacy document rows still exist in the
> database, but nothing imports through them anymore.

---

## Highlights

| | |
|---|---|
| **Bank sync** | Config-driven Pluggy accounts (Nubank, Caixa, C6…), incremental import, refund netting, installment series |
| **AI categorization** | Batch proposals reviewed one-by-one in a modal or applied in bulk; "nova: X" creates a category on the fly |
| **Learning memory** | Every category/tag decision is recorded in an append-only change log and fed back into the AI prompts |
| **Charts** | Category donut, 12-month trend, daily stacked bars, GitHub-style spending heatmap, top tags |
| **Forecast** | Future installments + recurring obligations projected 30–180 days ahead |
| **Recurring rules** | Alias-based matching that links rules to real items — analytics only, never double-counts |
| **Tags** | Normalized (lowercase, accent-stripped), with registry descriptions injected into prompts |
| **Monthly digest** | Per-month AI narrative (resumo, destaques, avisos) generated on demand |
| **Single-user auth** | One argon2 password + signed session cookie, rate-limited login |

## Screens

| Route | Page | What it does |
|---|---|---|
| `/` | **Gráficos** | Summary cards, category donut, 12-month trend, daily spend by category, calendar heatmap, top tags — all honoring the shared filters |
| `/forecast` | **Previsão** | Horizon selector (30–180 days), monthly recurring cost, expected spend, stacked forecast chart (parcelas vs recorrentes), upcoming breakdown table |
| `/lista` | **Lista** | Full transaction history (500-row cap) with search/date range/multi category/multi tag/bank/kind/installment filters, bulk edit, AI tagging, manual recurring-rule linking, refund chips |
| `/memory` | **Memória** | Tabs: **Categorias** (CRUD), **Tags** (usage counts, rename, merge, delete, descriptions), **Histórico** (change log), **Diário** (free-form notes) |
| `/recurring` | **Recorrentes** | Rules with alias/isolated-case name entries, derived tags (with conflict flag), recent occurrences, monthly cost, link-to-item flow |
| `/system` | **Sistema** | Pluggy integration (accounts, sync, forced re-sync of a period) + DB stats and items-by-status |
| `/login` | — | Single-password login (default dev password: `deepsave`) |

`/categories`, `/tags` and `/pluggy` redirect to their merged pages.

---

## Quick start (Docker, recommended)

Builds backend + frontend into one image and starts it alongside Postgres:

```bash
cp .env.example .env        # set SESSION_SECRET, APP_PASSWORD, DEEPSEEK_API_KEY…
docker compose up --build   # full app at http://localhost:8080
```

- The backend serves the built SPA, so **one container, one port**.
- Log in with the password from `APP_PASSWORD` (default dev password: `deepsave`).
- `docker compose up --build` requires `SESSION_SECRET` and `APP_PASSWORD` in `.env`
  (the compose file fails loudly if they're missing).

For an edit-and-reload loop use [`docker compose watch`](https://docs.docker.com/compose/how-tos/file-watch/):
it watches the sources and rebuilds/recreates the `app` container on every save
(BuildKit reuses the cached Rust/node dependency layers, so only the changed
stage recompiles).

```bash
docker compose watch
```

## Configuration

All settings are environment variables (see [`.env.example`](./.env.example)):

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | — (required) | Postgres connection string |
| `PORT` | `8080` | HTTP port |
| `SESSION_SECRET` | dev fallback | Cookie-signing key — **must be ≥ 32 bytes** (guarded at startup) |
| `COOKIE_SECURE` | `false` | Set `true` once served over HTTPS (adds `Secure` to the session cookie) |
| `APP_PASSWORD` / `APP_PASSWORD_HASH` | `deepsave` | Login password — plaintext (hashed at startup) or pre-computed argon2 hash; hash preferred in production |
| `STATIC_DIR` | `./frontend/dist` | Built SPA served by the backend |
| `STORAGE_DIR` | `./storage` | Legacy document storage (volume `appdata` in compose) |
| `COVERAGE_MONTHS` | `12` | Coverage window tracked per source |
| `DEEPSEEK_API_KEY` | — | DeepSeek API key |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | DeepSeek endpoint |
| `DEEPSEEK_MODEL` | `deepseek-v4-flash` | Chat/categorization model |
| `DEEPSEEK_INPUT_PRICE_PER_M` / `DEEPSEEK_CACHE_HIT_PRICE_PER_M` / `DEEPSEEK_OUTPUT_PRICE_PER_M` | `0.27` / `0.07` / `1.10` | Token prices (USD per 1M) used for `ai_calls` cost accounting |
| `PLUGGY_API_KEY` | — | Pluggy auth (API-key mode — see below) |
| `PLUGGY_CLIENT_ID` / `PLUGGY_CLIENT_SECRET` | — | Pluggy auth fallback (client-credentials mode) |
| `PLUGGY_ACCOUNTS` | — | JSON array of accounts to import (see below) |
| `DAILY_PLUGGY_SYNC` | `true` | Set `false` to disable the automatic daily Pluggy sync |

### Login & security

- Auth is a single password (argon2) with a signed `deepsave_session` cookie
  (`HttpOnly`, `SameSite=Lax`, `Secure` when `COOKIE_SECURE=true`).
- Only `/api/health`, `/api/auth/login`, `/api/auth/logout`, `/api/auth/me` and the
  login-page assets are unauthenticated; everything else under `/api` requires a session.
- Login is rate-limited in-app (20 attempts/min per peer); a reverse proxy
  (see `deploy/`) can add an IP-level limit on top.

## Pluggy (open-banking) integration

Transactions are pulled straight from the banks — no files, no uploads. The UI lives
in **Sistema** (`/system`); the old `/pluggy` route redirects there.

1. **Accounts are config-driven.** Pluggy account ids are stable, so they're read once
   from the Pluggy dashboard and listed in `PLUGGY_ACCOUNTS`:

   ```json
   [{"id":"<uuid>","bank":"nubank","kind":"BANK","name":"Nubank - Conta"},
    {"id":"<uuid>","bank":"nubank","kind":"CREDIT","name":"Nubank - Cartão"}]
   ```

   `bank` is one of `nubank|caixa|c6`, `kind` is `BANK|CREDIT` (drives sign conventions).
   Accounts are seeded into `pluggy_accounts` at startup and re-seeded on every sync.

2. **Auth is API-key first.** `PLUGGY_API_KEY` is sent directly as `X-API-KEY` and never
   refreshed (ideal for single-user setups). `PLUGGY_CLIENT_ID`/`PLUGGY_CLIENT_SECRET`
   are the fallback (JWT `/auth` flow, cached 90 min, auto-refreshed on 401).

3. **Sync** (`POST /pluggy/sync`) pulls `/v2/transactions` (cursor-paginated) per account
   and imports them as `confirmed` items (`source = 'pluggy'`), idempotent via
   `items.external_id` (partial unique index) — existing rows are never touched.
   - **Incremental by default**: only transactions newer than the account's last imported
     date (minus a 3-day overlap for late-posted charges).
   - **Forced full re-pull**: pass `?from=&to=` (YYYY-MM-DD) — the Pluggy UI has date
     inputs for this.
   - A **daily automatic sync** (last 7 days) runs ~45 s after startup and every 24 h
     (`DAILY_PLUGGY_SYNC=false` disables it).

4. **Post-processing on every sync**:
   - `link_refunds` pairs each refund with the expense it reverses (exact |amount| +
     merchant tokens first, partial second) — linked refunds **net their expense to
     zero** in every chart.
   - `assign_installment_series` groups installments into `purchase_series` so the
     forecast can project future parcels.
   - The **MCC → category rule** deterministically categorizes uncategorized card items
     (5411 → Supermercado, 5812 → Restaurantes, 5542 → Transporte, 5912 → Saúde,
     5817/5818/5968 → Assinaturas, retail → Outros…).

5. **Sign conventions**: bank DEBIT → negative expense, CREDIT → positive income;
   card charges (positive DEBIT) → negative expense with `installment/installment_count`
   from `creditCardMetadata`; card-side payments/refunds are skipped; bank-side
   "Credit card payment" becomes `internal` (already on the card side).

## How the AI works

- **Proposals** (`services/ai_tags.rs`): select items in Lista → "Taggear com IA" → a
  background worker sends a compact payload (existing categories/tags, item rows, and per
  merchant the last tagged examples) to DeepSeek. Batches carry a `kind`:
  `tags` | `categorize` | `full` (category + tags in one call).
- **Review**: suggestions are reviewed one-by-one in a modal (per-field apply checkboxes,
  editable category dropdown incl. "sem categoria" — clears the category — and "nova: X" —
  creates a new one) or in bulk from the amber banner (Aplicar/Ignorar todas, "Só itens
  com sugestão" filter).
- **Prompts always receive**: the category list, tag descriptions (from the `tags`
  registry), the last 5 change-log entries per merchant, tagged examples, and Pluggy
  metadata (`pc`/`mcc`/`op`/`pay`). Tags that are also category names are never suggested.
- **Learning**: every category/tag decision you make (item edit, bulk edit, AI apply,
  memory apply) is appended to the **change log** (`/memory` → Histórico). The AI learns
  *only* from this log — the old `merchant_memory` table was decommissioned (its rows
  became `source='legacy'` entries).
- **Monthly digest**: on Gráficos, a full-month view offers generating a saved AI narrative
  (`resumo`, `destaques`, `avisos`) via `/api/dashboard/digest`.
- Every AI call is recorded in the `ai_calls` table (model, tokens, cost, purpose).

## Data model & conventions

- **Amounts** are integer cents; **negative = expense, positive = income**.
- **Refund netting**: a refund linked to its expense is treated as a negative expense
  bucketed by the *charge's* month/category/merchant — nothing inflates, nothing
  double-counts.
- **`card_payment`** (credit-card bill payments) and **`internal`** (transfers between
  your own accounts) are excluded from spend/income.
- **Recurring rules are analytics-only** — they never auto-create items (statement items
  are the single source of truth). Matching is alias-based (normalized exact equality
  against `recurring_aliases`, tolerating a trailing amount); manual links
  (`POST /items/{id}/link-recurring`) win over automation; `next_due_on` is never in the past.
- **Installments** live in `purchase_series`; with `installments=first_only` the first
  parcel shows the whole price (`amount_cents × installment_count`).
- **Categories are intrinsic to a merchant; tags are situational.** Tag
  rename/merge/delete cascade to items, recurring rules and the tags registry.
- **Migrations** are embedded at compile time (`sqlx::migrate!`) and auto-applied at
  startup; `backend/migrations/` must exist when building (including in Docker).
- **Legacy documents**: tables/rows remain from the decommissioned upload pipeline, but
  nothing imports through them anymore.

## Development

```bash
# 1. start postgres
docker compose up -d postgres

# 2. backend (runs migrations on startup)
cp .env.example .env   # adjust values
cd backend && cargo run       # http://localhost:8080/api/health

# 3. frontend (separate terminal)
cd frontend && npm install && npm run dev   # http://localhost:5173
```

The Vite dev server proxies `/api` to the backend on port 8080. The backend also serves
the built SPA from `STATIC_DIR` with an `index.html` fallback — keep `/api/*` routes
above the fallback.

### Testing

```bash
cd backend && cargo test    # backend tests (needs a Postgres reachable via DATABASE_URL)
cd frontend && npm run build  # type-check + build frontend
```

- Integration tests use `#[sqlx::test]` + `wiremock` (fake Pluggy/DeepSeek APIs); the
  Pluggy mapping/dedupe matcher has unit tests in `services/pluggy.rs`, and the full
  import pipeline is exercised in `tests/pluggy.rs`.
- CI (GitHub Actions) runs `cargo check` + `cargo test` against a Postgres service and a
  production frontend build on every push/PR.

### Apply migrations manually (optional)

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
cd backend && sqlx migrate run
```

## Seed fake data

Populates the database with fake categories, accounts, a 3-month spending tree,
transfers, income and recurring rules (useful while testing the UI):

```bash
./scripts/seed.sh
```

Runs from the host via `docker exec` into the Postgres container (override the
container name with `DEEPSEED_PG_CONTAINER` if needed). Re-running it resets and
re-seeds the data tables.

## Backup & restore

Postgres runs in the `deepsave-postgres` container (user/db `deepsave`); legacy uploaded
files live in the `appdata` volume (`/app/storage`), **not** in the database.

```bash
./scripts/backup                 # writes ./backups/deepsave-<timestamp>.dump (+ -appdata.tar.gz)
./scripts/restore                # restores the most recent backup; or pass a timestamp:
./scripts/restore 20250101-120000
```

Both accept `DEEPSAVE_BACKUP_DIR`, `DEEPSAVE_PG_CONTAINER`, `DEEPSAVE_PG_USER`,
`DEEPSAVE_PG_DB`, `DEEPSAVE_APPDATA_VOLUME` env overrides (see headers in the
scripts). Restore **overwrites** the current database and restarts the app.

## Deployment & remote access

The repo ships a complete remote-access setup in [`deploy/`](./deploy/README.md):
a **WireGuard tunnel** from your home machine (where DeepSave runs, behind NAT —
*no port-forwarding needed*) to an always-up VPS that reverse-proxies
`ds.cuchi.me` over HTTPS with an offline fallback page.

- VPS: WireGuard server (`wg0`), nginx (security headers, per-IP rate limiting,
  20 MB body cap, offline fallback) — reference configs in `deploy/vps/`.
- Home: WireGuard client (`deepsave.conf`), `PersistentKeepalive` keeps the NAT
  mapping fresh — survives home IP changes.
- Read the full walkthrough (DNS, key generation, firewalld, troubleshooting)
  in [`deploy/README.md`](./deploy/README.md).

## API overview

Base: `/api` (JSON, snake_case field names; errors are `{ "error": ... }`). Everything
except the auth/health routes requires the session cookie.

| Area | Endpoints |
|---|---|
| Auth | `POST /auth/login`, `POST /auth/logout`, `GET /auth/me`, `GET /health` |
| Items | `GET/POST /items`, `PATCH /items/bulk`, `GET /items/summary`, `GET/PATCH/DELETE /items/{id}`, `POST /items/{id}/link-recurring`, `POST /items/link-recurring`, `GET /banks` |
| Categories / Tags | `GET/POST /categories`, `DELETE /categories/{id}`, `GET /tags`, `GET /tags/usage`, `GET /tags/registry`, `PATCH /tags/{tag}`, `PATCH /tags/rename`, `POST /tags/merge`, `DELETE /tags/{tag}` |
| Memory / History | `GET /change-log`, `GET/POST /diary`, `PATCH/DELETE /diary/{id}` |
| AI tags | `POST /ai-tags/batches`, `GET /ai-tags/batches`, `GET /ai-tags/suggestions`, `POST /ai-tags/suggestions/{id}/apply\|dismiss`, `POST /ai-tags/suggestions/apply-all\|dismiss-all` |
| Recurring | `GET/POST /recurring`, `PATCH/DELETE /recurring/{id}`, `GET /recurring/{id}/occurrences`, `GET /recurring/merchants`, `GET /recurring/merchant-profile`, `GET /recurring/monthly-cost` |
| Dashboard | `GET /dashboard`, `GET /dashboard/trend`, `GET /dashboard/daily`, `GET /dashboard/tags`, `GET /dashboard/forecast`, `GET /dashboard/upcoming`, `GET/POST/DELETE /dashboard/digest` |
| System / Pluggy | `GET /system`, `GET /pluggy/status`, `GET /pluggy/accounts`, `POST /pluggy/sync` |

Shared filters on items/dashboard routes: `search`, `date_from`/`date_to`,
`category_ids` (multi, OR), `tags` (multi, OR, `__none` sentinel), `bank`, `kind`,
`installments` (`all`/`first_only`/`only`), `limit` (list, default cap 500).

## Docs

- [`PLAN.md`](./PLAN.md) — remaining work (categories tree UI, usage endpoint, …)
- [`PLAN-DONE.md`](./PLAN-DONE.md) — completed milestones M0–M21
- [`AGENTS.md`](./AGENTS.md) — engineering conventions & gotchas (sqlx 0.9, auth, Pluggy…)
