# DeepSave — Remaining Plan

> v0.8 — only remaining work. Completed work is archived in [`PLAN-DONE.md`](./PLAN-DONE.md).

> M15 (memory overhaul: tags in memory, preview-before-apply, merged page) is done — see PLAN-DONE.md.

> Forecast split (Gastos × Previsão) is **done** (M19) — see PLAN-DONE.md.

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
