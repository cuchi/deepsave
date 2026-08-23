# DeepSave — AI-Augmented Personal Finance Manager

> Plan document v0.4 — for review before implementation.

## 1. Vision

DeepSave is a **single-user, self-hosted** personal finance web app. You upload raw
financial documents (bank & credit-card statements, receipts, payment slips), and the
app — with help from **DeepSeek** — turns them into a structured, searchable,
monthly-organized tree of spending.

The core idea is the **spending tree**:

```
Credit card statement (source document)
└── R$ 100.00 — Supermercado XYZ (statement line item)
    └── R$ 40.00 — Groceries        (receipt line item)
    └── R$ 25.00 — Cleaning supplies (receipt line item)
    └── R$ 35.00 — Drinks            (receipt line item)
```

A high-level charge from a statement can be *broken down* by uploading the matching
receipt. The app (via DeepSeek) proposes the links and categories; you confirm or fix
them.

---

## 2. Goals & Non-goals

### Goals
- Single-user with password-only login.
- Ingest four document types: **card statements**, **bank statements**, **receipts**, **payment slips**.
- AI-assisted identification, tagging, categorization, and linking of items.
- Monthly organization with charts and indicators.
- Hierarchical (tree) spending: parent items (statement charges) → child items (receipt lines).
- Human-in-the-loop: AI *proposes*, the user *confirms/corrects*.
- Fully manual entry: add/edit/delete spend items by hand whenever needed.
- Handle transfers between own accounts (detect, exclude from spend, pair the two legs).

### Non-goals (v1)
- Multi-user / multi-account / team features.
- Automatic bank API integrations (Plaid-like / Open Banking). Manual file upload only.
- Budgeting (planned for post-MVP, not in v1).
- Mobile apps (responsive web only).
- Multi-currency accounting (amounts stored with currency code, but no FX logic).

---

## 3. Requirements (user + filled gaps)

### Functional
| # | Requirement | Notes |
|---|-------------|-------|
| F1 | Password-only login | One user, one password. Session cookie. |
| F2 | Upload documents | Card statements, bank statements, receipts, payment slips. Formats: PDF, images (JPG/PNG), and **CSV** (statements). |
| F3 | Document processing | Extract text via PDF parsing or OCR; classify document type. |
| F4 | AI extraction | DeepSeek returns structured line items (date, merchant, amount, description, category, tags). |
| F5 | Tree structure | Items can have parent/child relationships (statement → receipt lines). |
| F6 | Linking | AI suggests matching receipt line items to statement items (merchant + date + amount). |
| F7 | Monthly view | Group items by month; totals, category breakdown, indicators. |
| F8 | Charts | Spend by category, trend over months, top merchants, receipt coverage. |
| F9 | Review workflow | Extracted items enter a "pending review" state before becoming confirmed. |
| F10 | Search / filter | By merchant, tag, category, description, amount range. |
| F11 | Manual items | Create/edit/delete spend items by hand, attach to a parent, link manually. |
| F12 | AI usage tracking | Track DeepSeek tokens (incl. cache hits) and estimated cost per call/document. |
| F13 | Categorization memory | Persist learned merchant→category/tag rules from user confirmations and reuse them in prompts. |
| F14 | Transfers | Detect transfers between own accounts; exclude from spend; pair the out/in legs. |
| F15 | Own accounts | Register the 3 own bank accounts (bank + account number) to drive transfer detection. |
| F16 | Installments | Credit-card `Parcela` (e.g. 7/10) is captured and shown; each installment is a dated item; **future installments are auto-created**. |
| F17 | Income tracking | Income items (salary, "Pagamento recebido", incoming transfers) are tracked and shown, but kept out of spend totals. |
| F18 | Recurring items | Detect + track items that repeat monthly/yearly (subscriptions, bills); suggest after 2 repeats (confirmation required); show upcoming; auto-create next occurrences a few days ahead. |

### Filled gaps (assumptions to confirm)
- **Amounts**: store in cents (integer) + ISO 4217 currency code. Primary currency **BRL**,
  but the model doesn't hardcode it. Sign conventions differ per bank (e.g. Nubank card
  uses negative for inflow) — we normalize to one convention: **negative = expense, positive = income**.
- **Card statements**: PDF **and CSV**. CSV is the primary format for **Nubank, Caixa, C6**
  exports; each has its own column layout and we map them to a common schema. Observed in
  `examples/`:
  - **Nubank credit card** (`Nubank_2026-08-15.csv`: `date,title,amount` — ISO date, comma decimals, negative = inflow).
  - **Nubank bank statement** (`NU_32530067_01AGO2026_22AGO2026.csv`: `Data,Valor,Identificador,Descrição` — DD/MM/YYYY, signed amounts, UUID identifier, rich Pix/boleto descriptions).
  - **C6 credit card** (`Fatura_2026-08-15.csv`: `Data de Compra;Nome no Cartão;…;Categoria;Descrição;Parcela;Valor (em US$);Cotação (em R$);Valor (em R$)` — semicolon-separated, includes `Parcela` installments and a USD/BRL split).
  - **Caixa bank statement** (`comprovante2026-08-23_185105.pdf`: **PDF only** — Caixa has no CSV export; we parse/OCR the PDF).
  (Nubank and C6 have CSV exports; PDF remains the fallback path.)
- **Payment slips (boletos)**: treated as standalone spend items (not linked to card statements).
- **Receipts**: mostly **photos (JPG/PNG)** → OCR is the main path. Digital PDFs (NFC-e) still supported via text extraction.
- **Language**: documents and AI prompts are **pt-BR**.
- **Privacy**: document text is sent to DeepSeek by default (user trusts the provider).
- **Categories**: a built-in editable category tree (e.g. Groceries, Transport, Health…). AI maps to it, can suggest new ones.
- **Tags**: free-form, AI-suggested (e.g. `weekly`, `trip-sao-paulo`, `recurring`).
- **Matching**: a receipt is linked to a statement item using **both** heuristics together: fuzzy merchant-name similarity, a date window, and an amount tolerance (sensible defaults: ±3 days, amount within 5%, merchant similarity > 0.8 — all configurable). If unmatched, the receipt becomes its own root item.
- **Own accounts & transfers**: you register your 3 accounts (bank + number). Transfers
  between them (Pix/TED/DOC) are detected via description patterns + counterparty match,
  flagged `transfer_out`/`transfer_in`, **excluded from spend totals**, and paired (out leg
  ↔ in leg) by amount + date (+ bank identifier/UUID when present).
- **Recurring items**: some items repeat monthly/yearly (subscriptions, bills). We detect
  them (same merchant + similar amount + regular interval), suggest a `recurring_rules`
  record **after 2 repeats**, and **require user confirmation** before enabling it. Once
  active, the next occurrence is auto-created (as `pending_review`) **a few days before**
  its due date (default 3 days, configurable).
- **Duplicate detection**: by document hash + fuzzy item match, to avoid double-counting.

---

## 4. Architecture Overview

```
┌─────────────────────────────┐
│        Browser (React)      │
│  Dashboard / Month / Upload │
└──────────────┬──────────────┘
               │ HTTPS / JSON
┌──────────────▼──────────────┐
│        Backend (Rust)       │
│  Axum web server            │
│  ┌────────────────────────┐ │
│  │  Ingestion pipeline    │ │   PDF text / CSV parsing
│  │  OCR (Tesseract)       │ │   Receipt photo & scan OCR
│  │  AI service (DeepSeek) │ │   Structured extraction + linking
│  │  Matching/linking      │ │   Merchant/date/amount heuristics
│  │  Memory                │ │   Learned merchant→category rules
│  └────────────────────────┘ │
│  SQLx (PostgreSQL)          │
└──────────────┬──────────────┘
               │
      ┌────────▼────────┐
      │   PostgreSQL    │   documents, items (tree), categories, tags, ai_calls, merchant_memory, accounts, recurring_rules
      └─────────────────┘
      ┌─────────────────┐
      │  File storage    │   raw uploaded files (local dir or S3-compatible)
      └─────────────────┘
      ┌─────────────────┐
      │  DeepSeek API    │   api.deepseek.com (OpenAI-compatible)
      └─────────────────┘
```

Processing is **asynchronous**: upload returns immediately, a background worker
queue processes the document, and the frontend polls for status.

---

## 5. Technology Choices

### Backend (Rust)
| Concern | Choice | Rationale |
|---------|--------|-----------|
| Web framework | **Axum** (tokio) | De-facto modern choice, great ecosystem, typed routing, multipart support. |
| Async runtime | **tokio** | Required by Axum. |
| Database access | **SQLx** (async, compile-time-checked queries) | Type-safe SQL, migrations built-in. |
| Database | **PostgreSQL 16** | Recursive CTEs for tree queries, JSONB for AI payloads, solid for structured finance data. |
| Migrations | **sqlx-cli** migrations | Versioned schema, works in CI. |
| Password hashing | **argon2** (via `argon2` crate) | State-of-the-art KDF. |
| Sessions | **tower-cookies** + signed session cookie | Simple for single user; no JWT complexity needed. |
| PDF text | **pdf-extract** (or `lopdf` fallback) | Text extraction from digital PDFs. |
| OCR | **tesseract-rs** (wraps system `tesseract` + `leptonica`) | Free fallback for receipt photos/scans when the vision model isn't used; also renders PDF pages to images with **pdfium**/`mupdf`. |
| CSV parsing | **csv** crate (+ per-bank column mappers) | Parses Nubank/Caixa/C6 card statement exports. |
| HTTP client | **reqwest** | Calls DeepSeek API. |
| Serialization | **serde** + **serde_json** | JSON DTOs. |
| Config | **figment** or `dotenvy` + env vars | 12-factor config. |
| Validation | **validator** crate | Input validation. |
| Job queue | **SQLx-backed queue** (custom, simple) | Avoid Redis dependency for single user; or **redis + apalis** if we want to scale. Start simple. |
| Logging | **tracing** + **tracing-subscriber** | Structured logs. |
| Errors | **anyhow** (internal) + typed `AppError` → JSON | Clean error responses. |

> **Decision**: **PostgreSQL** confirmed. The schema uses JSONB and recursive CTEs,
> which Postgres handles well. (SQLite was considered but rejected.)

### Frontend (React)
| Concern | Choice | Rationale |
|---------|--------|-----------|
| Build tool | **Vite** | Fast, standard. |
| Language | **TypeScript** | Type safety end-to-end. |
| UI framework | **React 18** | Requirement. |
| Styling | **Tailwind CSS** + **shadcn/ui** | Fast, consistent, copy-paste components. |
| Routing | **React Router** | Standard. |
| Data fetching | **TanStack Query** | Caching, polling for processing status, mutations. |
| Forms | **react-hook-form** + **zod** | Typed forms; zod schemas mirror backend. |
| Charts | **Recharts** | Declarative, good defaults for pie/bar/line. |
| Tree UI | custom component (or `react-arborist`) | Expandable spending tree. |
| File upload | **react-dropzone** | Drag-and-drop. |
| HTTP client | **axios** or native fetch wrapper | Simple JSON client with cookie auth. |

### AI
- **Provider**: DeepSeek, OpenAI-compatible chat completions API.
  - Base URL: `https://api.deepseek.com`
  - Models (as available on the API):
    - `deepseek-v4-flash` — default for extraction/classification/linking (fast, cheap).
    - `deepseek-v4-pro` — fallback for hard linking/reconciliation (higher quality).
    - `deepseek-v4-flash-vision-exp` — **vision** model for direct image understanding (receipt photos).
  - **Vision**: receipt *photos* are sent directly to the vision model for structured
    extraction (no OCR step). **Tesseract OCR stays as a free fallback** for when the
    vision model is unavailable; scanned PDFs are rendered to images and can go either
    route (vision first, OCR fallback).
- **JSON mode**: request `response_format: { "type": "json_object" }` and enforce a strict
  JSON schema in the prompt; validate + repair output with `serde_json` and a retry loop.
- **Context caching**: DeepSeek's API caches the prompt prefix on disk automatically and
  returns `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` in `usage`. We exploit
  this by keeping a **large, stable system prefix** (instructions + category list +
  categorization memory) and putting the variable document text at the end — so repeated
  calls hit the cache and cost ~4× less on input tokens.
- **Usage tracking**: every call records prompt/completion/cache tokens and an estimated
  cost (computed from configurable price rates) into the `ai_calls` table (§6).

### AI cost tracking
- **Estimated cost (recommended)**: each API response's `usage` already gives us exact
  token counts (`prompt_tokens`, `completion_tokens`, `cache_hit_tokens`, `cache_miss_tokens`).
  We multiply those by per-token prices configured in env vars → a USD estimate stored in
  `ai_calls.cost_usd`. Accurate as long as our configured prices match DeepSeek's current
  pricing; it drifts if prices change, or if your account has credits/free allowances.
- **Exact billing**: what DeepSeek *actually charges* your account. The chat-completions
  API does **not** expose per-call billing; the closest option is polling an account
  balance/usage endpoint (if available) to reconcile totals — but that's coarse, not per call.
- **Recommendation**: store exact token counts (free) in `ai_calls`, compute *estimated*
  cost from configurable rates, and optionally reconcile against the account balance
  endpoint periodically. Good enough for a single user.

---

## 6. Data Model

### Tables

```sql
-- Own bank accounts (drives transfer detection + per-account views)
accounts (
  id             uuid PK,
  name           text NOT NULL,          -- e.g. 'Nubank', 'Caixa', 'C6'
  bank           text,                   -- bank name / code
  account_number text,                   -- masked or full, user-entered
  created_at     timestamptz NOT NULL DEFAULT now()
);

-- Uploaded files
documents (
  id            uuid PK,
  kind          text NOT NULL,            -- 'card_statement' | 'bank_statement' | 'receipt' | 'payment_slip'
  account_id    uuid NULL REFERENCES accounts(id) ON DELETE SET NULL,  -- which own account this doc belongs to
  filename      text NOT NULL,
  content_type  text NOT NULL,
  sha256        text NOT NULL UNIQUE,     -- dedupe
  file_path     text NOT NULL,            -- storage location
  status        text NOT NULL,            -- 'pending' | 'processing' | 'needs_review' | 'processed' | 'failed'
  error_message text,
  ocr_text      text,                     -- extracted raw text (for debugging)
  ai_payload    jsonb,                    -- raw DeepSeek response
  uploaded_at   timestamptz NOT NULL DEFAULT now(),
  processed_at  timestamptz
);

-- DeepSeek API call accounting (tokens, cache hits, cost)
ai_calls (
  id                uuid PK,
  document_id       uuid NULL REFERENCES documents(id) ON DELETE SET NULL,
  purpose           text NOT NULL,        -- 'extract' | 'link' | 'classify'
  model             text NOT NULL,
  prompt_tokens     integer NOT NULL,
  completion_tokens integer NOT NULL,
  total_tokens      integer NOT NULL,
  cache_hit_tokens  integer NOT NULL DEFAULT 0,   -- DeepSeek prompt_cache_hit_tokens
  cache_miss_tokens integer NOT NULL DEFAULT 0,   -- DeepSeek prompt_cache_miss_tokens
  cost_usd          numeric(12,6) NOT NULL DEFAULT 0,
  duration_ms       integer,
  status            text NOT NULL,        -- 'ok' | 'error'
  error_message     text,
  created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_calls_doc_idx  ON ai_calls(document_id);
CREATE INDEX ai_calls_date_idx ON ai_calls(created_at);

-- Spending tree nodes (statement items, receipt lines, slips)
items (
  id             uuid PK,
  parent_id      uuid NULL REFERENCES items(id) ON DELETE CASCADE,
  document_id    uuid NULL REFERENCES documents(id) ON DELETE SET NULL,
  source         text NOT NULL,           -- 'card_statement' | 'bank_statement' | 'receipt' | 'payment_slip' | 'manual'
  kind           text NOT NULL DEFAULT 'expense',  -- 'expense' | 'income' | 'transfer_out' | 'transfer_in' | 'refund'
  status         text NOT NULL,           -- 'pending_review' | 'confirmed' | 'rejected'
  account_id     uuid NULL REFERENCES accounts(id) ON DELETE SET NULL,
  transfer_group_id uuid NULL,            -- links the two legs of a transfer (out ↔ in)
  installment       integer,              -- installment number (e.g. 7 of 10)
  installment_count integer,              -- total installments
  recurring_id    uuid NULL REFERENCES recurring_rules(id) ON DELETE SET NULL,
  occurred_on    date NOT NULL,           -- transaction date
  posted_on      date,                    -- statement posting date (if different)
  merchant       text,
  description    text NOT NULL,
  amount_cents   bigint NOT NULL,         -- signed, normalized: negative = expense
  currency       char(3) NOT NULL DEFAULT 'BRL',
  category_id    uuid NULL REFERENCES categories(id),
  tags           text[] NOT NULL DEFAULT '{}',
  raw_line       text,                    -- original line (statement row, receipt line, CSV row)
  match_confidence real,                  -- 0..1 from AI/heuristics
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX items_parent_idx ON items(parent_id);
CREATE INDEX items_month_idx  ON items(occurred_on, status);
CREATE INDEX items_doc_idx    ON items(document_id);
CREATE INDEX items_kind_idx   ON items(kind);
CREATE INDEX items_account_idx ON items(account_id);
CREATE INDEX items_transfer_idx ON items(transfer_group_id);

-- Editable category tree
categories (
  id         uuid PK,
  parent_id  uuid NULL REFERENCES categories(id),
  name       text NOT NULL UNIQUE,
  color      text,
  icon       text,
  is_active  boolean NOT NULL DEFAULT true
);

-- Suggested matches (AI proposals, confirm/reject)
matches (
  id            uuid PK,
  parent_item_id uuid NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  child_item_id  uuid NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  source         text NOT NULL,           -- 'ai' | 'heuristic' | 'manual'
  confidence     real NOT NULL,
  status         text NOT NULL,           -- 'suggested' | 'accepted' | 'rejected'
  created_at     timestamptz NOT NULL DEFAULT now()
);

-- Categorization memory: learned merchant → category/tag rules (feeds AI prompts)
merchant_memory (
  id                uuid PK,
  merchant          text NOT NULL UNIQUE,   -- normalized merchant name (lowercased)
  category_id       uuid NULL REFERENCES categories(id),
  tags              text[] NOT NULL DEFAULT '{}',
  confidence        real NOT NULL DEFAULT 0,   -- strengthened by each confirmation
  confirm_count     integer NOT NULL DEFAULT 0, -- times user confirmed/corrected
  last_confirmed_at timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now(),
  updated_at        timestamptz NOT NULL DEFAULT now()
);

-- Recurring items (subscriptions / regular bills)
recurring_rules (
  id            uuid PK,
  merchant      text,
  description   text NOT NULL,
  amount_cents  bigint NOT NULL,
  currency      char(3) NOT NULL DEFAULT 'BRL',
  category_id   uuid NULL REFERENCES categories(id),
  tags          text[] NOT NULL DEFAULT '{}',
  frequency     text NOT NULL,           -- 'weekly' | 'monthly' | 'yearly'
  interval      integer NOT NULL DEFAULT 1,  -- every N periods
  day_of_month  integer,                 -- for monthly (1-31)
  next_due_on   date,
  is_active     boolean NOT NULL DEFAULT true,
  source        text NOT NULL DEFAULT 'manual', -- 'manual' | 'ai' | 'detected'
  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now()
);

-- Simple user/session
users (
  id            uuid PK,
  password_hash text NOT NULL,
  created_at    timestamptz NOT NULL DEFAULT now()
);
-- sessions handled as signed cookie (stateless) — no table needed.
```

### Tree invariants
- A `receipt` item's `parent_id` points to a `statement` item.
- `statement` and `payment_slip` items are normally roots (`parent_id IS NULL`).
- Child items should sum to (at most) the parent amount. The UI shows **unallocated**
  remainder (parent − Σ children) so you see if a receipt didn't fully cover the charge.
- `transfer_out` / `transfer_in` items are **excluded from spend totals** and shown in a
  separate Transfers view (they are neutral movements between own accounts).
- Deleting a parent cascades to children.

---

## 7. Backend Design

### Modules (`backend/src/`)
```
main.rs            — bootstrap, config, router, state
config.rs          — env-based config (DB URL, DeepSeek key, storage path)
error.rs           — AppError → HTTP JSON
auth/              — password check, cookie session middleware
routes/            — axum handlers (thin)
  mod.rs
  auth.rs
  documents.rs
  items.rs
  categories.rs
  dashboard.rs
  matches.rs
  accounts.rs
  transfers.rs
services/
  ingest.rs        — orchestrate: extract → AI → persist → propose links
  extract.rs       — PDF text / CSV / OCR dispatch
  csv.rs           — per-bank CSV column mappers (Nubank, Caixa, C6)
  ai.rs            — DeepSeek client + prompts + JSON schema validation + usage tracking
  memory.rs        — merchant_memory maintenance + prompt context builder
  linking.rs       — merchant/date/amount matching heuristics
  transfers.rs     — transfer detection + out/in leg pairing
  recurring.rs     — recurrence detection + auto-creating future occurrences
  queue.rs         — SQLx-backed job queue worker
db/
  pool.rs
  migrations/      — SQL files
models/            — structs + FromRow + DTOs
```

### API Endpoints (v1)
| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/auth/login` | `{ password }` → sets session cookie |
| POST | `/api/auth/logout` | clears cookie |
| GET | `/api/auth/me` | current user info |
| POST | `/api/documents` | multipart upload (kind + file) → returns document |
| GET | `/api/documents` | list documents w/ status |
| GET | `/api/documents/:id` | document detail + extracted items |
| DELETE | `/api/documents/:id` | delete doc + cascade items |
| GET | `/api/items` | list/filter (month, category, merchant, tag, status, search) |
| GET | `/api/items/:id` | item + children |
| POST | `/api/items` | create manual item |
| PATCH | `/api/items/:id` | edit item (category, tags, amounts, parent) |
| DELETE | `/api/items/:id` | delete item + subtree |
| GET | `/api/items/:id/children` | subtree |
| GET | `/api/matches` | list suggested matches |
| POST | `/api/matches/:id/accept` | accept link |
| POST | `/api/matches/:id/reject` | reject link |
| GET | `/api/categories` | category tree |
| POST/PATCH/DELETE | `/api/categories` | CRUD |
| GET | `/api/dashboard?month=YYYY-MM` | aggregated indicators |
| GET | `/api/dashboard/trend?months=12` | monthly totals over time |
| GET | `/api/usage` | aggregated AI usage (tokens, cache hit rate, cost) over time |
| GET/POST/PATCH/DELETE | `/api/accounts` | manage own bank accounts |
| GET | `/api/transfers` | list transfers (paired + unpaired legs) |
| POST | `/api/transfers/:id/pair` | manually pair/unpair transfer legs |
| GET/POST/PATCH/DELETE | `/api/recurring` | manage recurring rules |
| GET | `/api/recurring/upcoming?months=3` | upcoming occurrences |

### Ingestion pipeline (per document)
1. **Receive** file → validate type/size → save to storage → insert `documents` (status `pending`) → enqueue job → return.
2. **Worker**:
   - Compute/extract text:
     - **CSV** → parse rows via the bank-specific mapper (Nubank/Caixa/C6) → canonical line format.
     - PDF with text layer → `pdf-extract`.
     - Receipt photo / scanned PDF → **vision model** (`deepseek-v4-flash-vision-exp`) for direct extraction; fallback to render + **Tesseract OCR** + text model.
   - Classify document type if not already known (AI or filename/hints).
   - **AI extraction**: send chunked text to DeepSeek with a strict prompt → JSON line items.
   - Persist items as `pending_review`, attach to `document_id`.
   - **Linking**: for receipt docs, find candidate statement items (same merchant, date
     within ±N days, amount ≤ parent, fuzzy string match) → create `matches` (status `suggested`).
   - **Transfer detection**: classify items as `transfer_out`/`transfer_in`/`refund` from
     description patterns (Pix/TED/DOC/transferência/estorno) + counterparty vs own accounts
     (AI assists on ambiguous cases). Pair out↔in legs using **both** amount + date window
     and the bank `Identificador`/UUID when present (exact UUID match wins; else amount+date)
     into a shared `transfer_group_id`.
   - **Installment expansion**: when an item has `installment_count > 1`, auto-create the
     remaining future installments (same day-of-month, +1 month each) as `pending_review`.
   - **Recurrence detection**: look for repeats across confirmed items (same merchant +
     similar amount + regular interval); after **2 repeats** suggest a `recurring_rules`
     record and **require user confirmation**; once active, auto-create the next occurrence
     as `pending_review` a few days before `next_due_on` (default 3 days).
   - Mark document `needs_review` (has pending items) or `processed`.
3. **User reviews** → accepts/rejects items and links → items become `confirmed`.
4. **Memory update**: on every confirmation/correction, upsert `merchant_memory`
   (normalized merchant → category/tags, bump `confirm_count`/`confidence`) so future
   prompts inherit the learning.

### DeepSeek prompts (draft shape)
- **Prompt layout for caching**: the system message is a **stable prefix** — global
  instructions + current category tree + top `merchant_memory` rules (as few-shot
  examples / "this merchant is usually category X"). The variable part (document text)
  goes last so the stable prefix is served from DeepSeek's disk cache.
- **Extraction prompt** (receipt / statement / slip): system prompt describes the JSON
  schema; user prompt contains the OCR/CSV text (or the image itself when using the
  vision model). Required output:
  ```json
  {
    "document_type": "receipt",
    "merchant": "Supermercado XYZ",
    "date": "2025-01-10",
    "total_cents": 10000,
    "currency": "BRL",
    "items": [
      {"description": "Groceries", "amount_cents": 4000, "category": "Groceries", "tags": ["food"], "kind": "expense", "installment": null, "installment_count": null}
    ]
  }
  ```
- **Linking prompt** (optional, for hard cases): given a statement item and N receipt
  items, return the best pairings + confidence.
- Guardrails: strict JSON, amounts as integers, unknown fields ignored, retry on
  parse failure (up to 2 retries with a "your output was invalid" follow-up).

---

## 8. Frontend Design

### Pages
| Route | Page | Purpose |
|-------|------|---------|
| `/login` | Login | Single password field. |
| `/` | Dashboard | Current month: total spend, category pie, trend line, top merchants, receipt coverage %, pending review count, income total, upcoming recurring. |
| `/months/:ym` | Month view | Tree of items for the month; expand statement → receipt lines; filters; **Add item** button for manual entry. |
| `/upload` | Upload | Drag-and-drop, choose doc kind (or auto-detect), progress, recent uploads + status. |
| `/review` | Review queue | Pending items and suggested matches; confirm/correct/reject. |
| `/categories` | Categories | Manage category tree, colors. |
| `/items/:id` | Item detail | Edit an item, view raw OCR text, see AI rationale. |
| `/transfers` | Transfers | List transfer legs, pair/unpair, see which are still unpaired. |
| `/recurring` | Recurring | List recurring rules, upcoming occurrences, enable/disable. |

### Key components
- `SpendingTree` — recursive expandable rows; each node shows description, amount,
  category chip, tags, confidence badge; inline editing.
- `UploadDropzone` — react-dropzone, multi-file, kind selector.
- `CategoryPie`, `TrendLine`, `TopMerchants`, `CoverageMeter`.
- `ReviewCard` — side-by-side: AI proposal vs editable form; accept/edit/reject.
- `ItemFormDialog` — manual add/edit of any item (description, amount, date, category, tags, parent, kind, account, installment).
- `StatusBadge` — pending/processing/needs_review/processed/failed.
- `RecurringCard` — a recurring rule with next due date, amount, and an on/off toggle.

---

## 9. Security

- Password hashed with **argon2id**; never stored in plaintext.
- Session cookie: `HttpOnly`, `SameSite=Lax`, `Secure` in production, signed
  (HMAC) so it can't be forged. Password change supported via env/config later.
- All routes (except login) behind auth middleware.
- File uploads: size limit (e.g. 20 MB), content-type + magic-byte validation,
  stored outside web root with randomized names; original filename sanitized.
- DeepSeek API key stored server-side only (env var), never sent to frontend.
- Rate-limit login attempts (simple in-memory/DB counter).
- Document text is sent to DeepSeek for processing by default (user trusts the provider).
  No other third parties receive data.

---

## 10. Project Structure

```
deepsave/
├── PLAN.md
├── README.md
├── .env.example
├── docker-compose.yml            # postgres (+ optional tesseract service)
├── Makefile                      # common tasks (dev, migrate, test)
├── backend/
│   ├── Cargo.toml
│   ├── migrations/
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── error.rs
│       ├── auth/
│       ├── routes/
│       ├── services/
│       ├── db/
│       └── models/
├── frontend/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.ts
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── api/
│       ├── components/
│       ├── pages/
│       └── lib/
└── storage/                      # uploaded files (gitignored)
```

---

## 11. Environment / Deployment

- **Target**: runs **locally**, but everything is **Dockerized anyway** for reproducible setup.
- **Dev**: `docker compose up` (Postgres), `cargo run` (backend), `npm run dev` (frontend via Vite proxy to backend).
- **Prod (local)**: `docker compose up` builds/runs backend + frontend + Postgres as
  containers; Tesseract installed as a system dependency (or baked into the backend image).
  Single `docker compose` command, no external cloud services.
- **Config env vars**:
  ```
  DATABASE_URL=postgres://...
  DEEPSEEK_API_KEY=sk-...
  DEEPSEEK_BASE_URL=https://api.deepseek.com
  DEEPSEEK_MODEL=deepseek-v4-flash
  DEEPSEEK_VISION_MODEL=deepseek-v4-flash-vision-exp
  DEEPSEEK_PRO_MODEL=deepseek-v4-pro
  DEEPSEEK_INPUT_PRICE_PER_M=0.27     # USD per 1M tokens (cache miss)
  DEEPSEEK_CACHE_HIT_PRICE_PER_M=0.07 # USD per 1M tokens (cache hit)
  DEEPSEEK_OUTPUT_PRICE_PER_M=1.10    # USD per 1M tokens
  # (prices are per-model in practice; keep a small map keyed by model if they diverge)
  STORAGE_DIR=./storage
  SESSION_SECRET=...
  APP_PASSWORD_HASH=...        # argon2 hash of the login password (bootstrapped)
  ```

---

## 12. Milestones

1. **M0 — Scaffolding**: repo layout, Axum hello-world, React+Vite+Tailwind skeleton, docker-compose, migrations, CI.
2. **M1 — Auth + core CRUD**: password login, session, categories, manual items (tree), month list.
3. **M2 — Upload + extraction**: file upload, storage, PDF text + OCR + CSV (Nubank/Caixa/C6), document status lifecycle.
4. **M3 — DeepSeek integration**: AI extraction to structured JSON, review workflow, retry/validation, `ai_calls` usage tracking, prompt caching.
5. **M4 — Linking + memory + transfers**: receipt → statement matching, match review UI, tree rendering with unallocated remainder, `merchant_memory` upserts + prompt injection, own-account registration, transfer detection/pairing.
6. **M5 — Dashboard & charts**: monthly aggregation endpoints + charts + indicators.
7. **M6 — Polish**: search/filter, duplicate detection, error handling, tests, docs, deployment.
8. **M7 — Recurring items**: recurrence detection, `recurring_rules` CRUD, upcoming occurrences, auto-create next occurrences.

Each milestone is independently shippable.

---

## 13. Testing Strategy

- **Backend**: unit tests for parsing/linking/matching logic; integration tests with
  `sqlx::test` against Postgres; DeepSeek calls mocked (record/replay fixtures).
- **Frontend**: Vitest + React Testing Library for components; MSW for API mocking.
- **Fixtures**: sample anonymized statements/receipts (text + images) in `backend/fixtures/`.

---

## 14. Resolved Decisions

1. **Recurring timing**: the next occurrence is auto-created **a few days before** `next_due_on`
   (configurable lead time, default 3 days) so it shows up in review before the due date.
2. **Recurring detection**: a `recurring_rules` suggestion is raised after **2 repeats** and
   **requires user confirmation** before the rule is enabled (no silent auto-creation).
3. **Caixa**: **PDF only** — no CSV export. Caixa bank statements are parsed/OCR'd from the PDF.

No open questions remain.

---

## 15. Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| DeepSeek returns malformed JSON | Strict schema prompt + `serde_json` validation + 2 retries + fallback to heuristic regex extraction. |
| Vision/OCR quality on receipts | Vision model is primary; OCR fallback for failures. Low-confidence extractions flagged for manual review; image preprocessing (contrast/deskew) for the OCR path. |
| Wrong AI categorization | Human-in-the-loop review queue; user corrections feed future prompt context (few-shot examples per merchant). |
| Double-counting | Document SHA256 dedupe + fuzzy item dedupe on ingest. |
| Tree sum mismatch | Show unallocated remainder; warn if children exceed parent. |
| Single-user lockout (forgot password) | Documented CLI/env reset (`APP_PASSWORD_HASH` regenerate). |
| Memory drifts / teaches wrong categories | Memory is advisory only (few-shot hints); user corrections overwrite; `confirm_count` gating (only inject rules with enough confirmations). |
| Cache misses make costs unpredictable | Keep the prompt prefix byte-stable; log `cache_hit_tokens` per call; dashboard shows hit rate. |
| CSV schema changes per bank | Per-bank mappers isolated in `services/csv.rs`; column-name tolerant matching + tests with fixtures. |
| Transfer misclassified as expense | AI + pattern detection + own-account matching; user can override `kind` in review; transfers excluded from spend by default. |
| Recurring false positives (one-off labeled recurring) | Detection is suggestion-only; requires user confirmation (configurable); rules have an `is_active` toggle. |
