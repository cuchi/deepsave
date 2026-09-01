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

### M11 — AI-assisted bulk tagging
Select items in the month view → “Taggear com IA” → a background batch (same worker
pattern as documents) sends a **compact payload** to DeepSeek: all existing tags, the
selected items (description, merchant, amount, date, category, tags) and, per merchant,
the last 3 confirmed tagged examples. Proposals land on the **Revisar** page, where each
item shows editable tag chips (add/remove, autocomplete) with `aplicar`/`ignorar` and
batch-level `aplicar tudo`/`ignorar tudo`. Applying merges the tags into the item (add
mode, deduped/normalized).
Backend: `ai_tag_batches` + `ai_tag_suggestions` tables, `services/ai_tags.rs` worker,
`/api/ai-tags/*` routes; AI call purpose `tag_batch` (recorded in `ai_calls`).

### M12 — Dashboard split: Gráficos × Lista
The dashboard was split into two pages: **Gráficos** (`/`, `/months/:ym` — month
selector + summary cards, pie, 12-month trend, top merchants, missing-sources alert)
and **Lista** (`/lista` — the item list over the **whole history**, capped at 500
rows via a new `limit` query param on `GET /api/items`, with search/filters/sort,
bulk edit and “Taggear com IA” in the same list). `MonthView` removed.

### M13 — Lista refinements
Filters are always expanded (no toggle). New **Parcelas** filter
(`installments` on `GET /api/items`: `all` / `first_only` — hides later
installments, keeping non-installments and the 1st parcel of each series — /
`only` — just installment items, all parcels) and a **date range** filter
(`date_from` / `date_to`, inclusive). Rows show the installment as
`x/y` after the name. New `GET /api/items/summary` (shared filter fragment,
roots-only net total + count) shown above the table: “Total: … · N itens”.

### M14 — Graphs get the filters
Shared `ItemFilters` component (search, date range, category, tag, bank, kind,
installments; sort is list-only) now drives both **Lista** and **Gráficos**. The
month selector is gone; the date range defaults to the **last complete month**.
`GET /api/dashboard` and `GET /api/dashboard/trend` accept the same filters
(shared `AGG_FILTERS` fragment; rejected items excluded; aggregation is
roots-only — receipt children no longer double-count spend). Trend window ends
at `date_to`'s month (rolling 12 months).

### M15 — Multi-select category & tag filters
Categorias and Tags filters support **multiple options** (OR semantics: item in
any selected category / carries any selected tag), on the Lista, the summary and
the graphs. Backend: `category_ids` / `tags` as comma-separated query params
(parsed in the shared `ITEM_FILTERS`/`AGG_FILTERS` fragments via
`cardinality(...) = 0 OR ...`; a `__none` sentinel adds "Sem categoria"/"Sem
tags"). Frontend: checkbox dropdowns (MultiSelect) replace the single selects;
URL params carry comma-joined values; one removable chip per selected
category/tag. With `installments=first_only`, the first parcel **shows the whole
price** (`amount_cents × installment_count`) in the list, the summary and every
graph aggregation.

### M17 — ECharts migration + new graphs
Switched the graphs page from Recharts to **ECharts** (tree-shaken
`echarts/core` + `CanvasRenderer`; recharts removed) with a modern dark look:
gradient area lines, donut pie with rounded segments, custom dark tooltips, faint
dashed grid. Kept the summary cards, pie and 12-month line. **New charts**
(expenses-only, roots-only, rejected excluded, honoring all filters):
- **Gastos diários por categoria** — stacked daily bar, top-8 categories + Outros
- **Calendário de gastos** — GitHub-style heatmap, fixed 12-month window ending
  at the range's end month (ECharts `calendar` + heatmap + visualMap)
- **Top tags** — horizontal bars, each tag = full spend carrying it (overlap
  allowed by design)
Backend: `GET /api/dashboard/daily?stack_by=category|none` and
`GET /api/dashboard/tags` (2 new integration tests).

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

### M16 — Memory overhaul: tags in memory + preview-before-apply + merged page

- `merchant_memory` now records **tags** (accumulate/union, first-occurrence order) alongside the
  category ("latest wins"); `record_confirmation` (item confirm/update/bulk) and AI tag suggestion
  apply (`ai_tags::apply_suggestion`) both feed it.
- `MemoryEntry` + memory API expose `tags` (create/update accept them, update replaces); AI
  extraction prompt memory lines include `merchant → category [tag1, tag2]` (gated `confirm_count ≥ 2`).
- Apply semantics: category replaces/clears; **tags are added** (union, never removed); per-item
  `apply-memory` (Lista/Review button) applies both.
- **Preview-before-apply**: `POST /memory/preview` lists exactly the items that would change
  (missing category and/or tags) with current→proposed diff; `POST /memory/apply` touches **only the
  selected ids** (transactional, idempotent). Blind `apply-all` / `apply-all-global` removed.
- Memory, Categories and Tags pages merged into a single `/memory` page (tabs); `/categories` and
  `/tags` redirect to it; nav has one "Memória" entry. "aplicar" (per row) and "Aplicar todas" open
  an **in-page preview panel** with checkboxes + select-all + "Aplicar selecionados".
- Tests: `backend/tests/memory.rs` (tag accumulation, preview candidates, apply only touches selection).

## Resolved decisions (from planning)

1. **Recurring timing**: next occurrence auto-created a few days before `next_due_on` (default 3 days).
2. **Recurring detection**: suggest after 2 repeats, require user confirmation.
3. **Caixa**: PDF only (no CSV export).

### M18 — Purchase series + forecast KPIs
- **Purchase series** (`purchase_series` + `items.series_id`): installments are linked
  across faturas by **re-parsing the stored documents** (the C6 fatura CSVs on disk /
  stored PDF text). `ParsedItem` now carries the original `purchase_date` (was
  discarded). Within a document, identical purchases split by purchase date; across
  documents, series match by (source, description, count) + monthly cadence + purchase
  date; ambiguous groups are skipped (never wrong, only conservative). `assign_document`
  runs at ingest; a startup `backfill` (idempotent) reprocesses legacy documents.
- **KPI — Recorrência mensal** (`GET /api/recurring/monthly-cost`): monthly-equivalent
  cost of active rules (weekly × 52/12, yearly /12), expenses only, ignores filters.
- **KPI — Gastos esperados** (`GET /api/dashboard/expected`): future installments of
  in-progress series + future recurring occurrences for a period, dated ≥ today
  (expenses only). Both KPIs shown as cards on Gráficos ("Gastos esperados" only when
  the period reaches the future). Live backfill: 283 items → 168 series.

### M19 — Gráficos split: Gastos × Previsão
Gastos (`/`) keeps all spent charts + full filters; the two KPI cards moved to the
new **Previsão** page (`/forecast`), which gained real forecast content:
- Horizon selector (30/60/90/180 dias, default 90) shared by all blocks.
- KPI cards: **Recorrência mensal** (global) + **Gastos esperados** (horizon; computed
  from the upcoming feed).
- **Monthly forecast chart** (ECharts stacked bar — parcelas vs recorrentes) via
  `GET /api/dashboard/forecast?months=N`.
- **Breakdown table** grouped by month with per-month subtotals, type badge
  (Parcela x/N / Recorrente), category, sorted by date, via
  `GET /api/dashboard/upcoming?days=N`.
- "Estimativa" note (equal parcels, C6-only detection, active rules).
Backend: shared `future_events` refactor (expected/forecast/upcoming all derive from
it); 2 new integration tests.

### M20 — Coverage: partial-month signalling for bank statements
Card faturas are always complete (user-confirmed); bank statements are verified by
their **covered period** — Nubank encodes it in the filename
(`NU_..._01AGO2026_22AGO2026.csv`), C6/Caixa in the header
(`Extrato de 24/08/2025 a 24/08/2026`). New `documents.statement_start/end`
(populated at ingest + a startup backfill), and the coverage endpoint now returns
`partial` months per source: a month is **partial** when a statement overlaps it
without covering it fully (◐ in the table), **present** when fully covered
(●), else absent. Faturas never partial. Strict DD/MM/YYYY round-trip parsing
(chrono %d/%Y are lenient — space padding, 3-digit years). Live: Nubank and C6
both flag Aug 2026 as partial.

### M21 — Recorrentes revamp (v0.9)

Design doc moved from `PLAN-recurring.md` (now archived here). Rules became pure
forecast/analytics constructs that **link to real items**:

- **Data model** (migration 0010): `recurring_rules.description` → `name` (free-form
  label, no matching role); `merchant` and stored `tags` dropped; new `recurring_aliases`
  table (`rule_id`, `name`, `is_alias`, partial unique index on `is_alias` — aliases are
  globally unique across rules); `items.linked_manually` marks user-made links so
  automation never destroys them.
- **Matching**: normalized exact equality (trim + lowercase + strip accents) against
  **alias** entries only; target = `merchant`, falling back to `description` only when
  merchant is null. At most one rule per item (alias uniqueness + single target).
  Excludes installments, receipt children, non-confirmed items. Alias matching tolerates
  a trailing amount in the item name (`matches_alias`) — one alias covers varying
  payments like "NETFLIX 22,90" / "NETFLIX 27,90". **Manual links win** over
  auto-match.
- **Linking runs** on item confirm, on ingest (confirmed items), on rule create/update
  (re-link), via manual `POST /items/{id}/link-recurring` (+ bulk `POST /items/link-recurring`).
- **Name entries**: two types — alias (auto-match) and isolated case (one-shot manual
  reference, may repeat across rules). Validation on save: name must exist in the data
  (item merchant/description), aliases must be globally unique → 400 with pt-BR message.
- **Derived rule tags**: `list` aggregates linked confirmed items' tags per rule (union)
  + `tags_conflict` flag (amber ⚠ when items disagree). Tag rename/merge/delete cascade
  to items only; no rule-side cascade (`TagRenameResult.recurring_updated` removed).
- **Next date never in the past**: `advance_next_due` computed at read time
  (`next_due_on` + `days_until`), clamped on write; old `GET /recurring/upcoming` folded
  into `list`.
- **New endpoints**: `GET /recurring/merchants?q=` (autocomplete), `GET /recurring/merchant-profile?name=`
  (auto-derivation for the add flow, `classify_gap` window suggestion),
  `GET /recurring/{id}/occurrences` (query by `items.recurring_id` — the link is the
  source of truth, no name-match fallback), `POST /recurring/reconcile`.
- **Suggestion feature removed** (was `suggest()` + `/recurring/suggestions` + Sugestões
  UI) — detection heuristics kept in git history; `classify_gap()` retained for the
  add-flow window suggestion.
- **Frontend**: single rules list (RuleCard with derived tags, name-entry editor with
  alias/isolated checkbox + validation, expandable "Ocorrências recentes"), add flow
  with autocomplete + preview (strict: read-only except name/window for unknown names),
  manual "Vincular a regra…" in the Lista ⋯ menu (item + bulk), recurring chip on linked
  items in the list.
- Tests: `backend/tests/recurring.rs` (validation, linking, relink, manual-link priority).
