# DeepSave — Remaining Plan

> v0.8 — only remaining work. Completed work is archived in [`PLAN-DONE.md`](./PLAN-DONE.md).

> M15 (memory overhaul: tags in memory, preview-before-apply, merged page) is done — see PLAN-DONE.md.

## Forecast: split Gráficos into Gastos × Previsão

> Decision: yes. Only worth it if Previsão gets real content (monthly chart +
> itemized breakdown), not just the two KPI cards. Series foundation (M18) already
> exists — `purchase_series` + `items.series_id` backfilled from documents.

### Steps

1. **Backend — `GET /api/dashboard/forecast?months=N`** (default 3)
   Per-month `{ month, installments_cents, recurring_cents, total_cents }` for the next N months —
   bucket the existing `expected_data` logic by month. Expenses only, dates ≥ today.
2. **Backend — `GET /api/dashboard/upcoming?days=N`** (default 90)
   Flat feed of the next obligations: `{ date, type: parcel|recorrente, description,
   category_name, amount_cents, progress (x/N for parcels) }`. Reuses `advance_next_due`
   + series tail. Expenses only.
3. **Backend tests** for both (reuse the `tests/series.rs` fixtures: young series + rules).
4. **Frontend — Previsão page (`/forecast`)**
   - Horizon selector: chips 30/60/90/180 dias (default 90), shared by all three blocks.
   - KPI cards: **Recorrência mensal** (global, unchanged) + **Gastos esperados** (horizon).
   - **Monthly chart**: next N months, ECharts stacked bar — parcelas vs recorrentes.
   - **Breakdown table**: upcoming feed grouped by month with per-month subtotals;
     type badge (Parcela x/N / Recorrente); sorted by date.
   - Small **"estimativa" note**: equal-parcel assumption; only C6 installments are
     detected today; based on active rules.
5. **Gastos page (`/`)**: keep all current charts + full filters; remove the two KPI
   cards (they move to Previsão).
6. **Route + nav**: add `/forecast` (nav "Previsão").

### Assumptions

- Forecast is **filter-free** (only the horizon) — it's a projection, not a slice.
- Expenses only. Instalments use the latest parcel amount × remaining parcels.
- Nubank/Caixa "N/M" installment detection is a separate parser gap (see loose ends).

## Categories & tags — remaining (#4 structure & tooling)

- Use the `categories.parent_id` tree in the UI (sub-categories; pie rolls up to parent).
- Category merge/rename tools + usage counts.
- Optional tag colors / color presets + emoji icon for categories.

> Done already: tag normalization, seeded categories, CSV bank-category mapping (#1),
> tag autocomplete + tags on rows + tag filter (#2), smarter matching + AI "new category"
> suggestion + one-click memory apply (#3), **tag management page** — usage counts, rename
> (cascades to items/recurring/memory), merge, delete (M9 ✓), **memory overhaul** — tags in
> memory + preview-before-apply + merged page (M15 ✓). The Categories tab on `/memory` keeps
> basic CRUD (create/delete) for now; the tree/merge/usage items above are the remaining gap.

## Loose ends (deferred, non-blocking)

| # | Item | Notes |
|---|------|-------|
| 1 | **Installment expansion** | Auto-create future installments when `installment_count > 1` (same day-of-month, +1 month each). Foundation exists: purchase series (M18). |
| 2 | **Receipt coverage %** indicator | Dashboard metric: how much statement spend is broken down by receipts. |
| 3 | **`/api/usage`** endpoint + UI | AI token/cost aggregation (data already recorded in `ai_calls`). |
| 4 | **Scanned PDF support** | Currently fails with "no text layer"; needs render-pages → vision/OCR. |
| 5 | **`deepseek-v4-pro`** | Wire up for hard linking/reconciliation (only flash + vision are used today). |
| 6 | **Amount-range filter** | `GET /api/items` has text/category/kind/tag filters but no `min/max` amount range. |

## Deferred / post-MVP (explicitly out of scope)

- Budgeting (per-category monthly budgets).
- Multi-currency accounting / FX logic.
- Mobile apps (responsive web only).
- Automatic bank API integrations (Open Banking / Plaid-like).
