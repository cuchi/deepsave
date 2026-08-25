# DeepSave — Remaining Plan

> v0.8 — only remaining work. Completed work is archived in [`PLAN-DONE.md`](./PLAN-DONE.md).

## M15 — Memory overhaul: tags in memory + preview-before-apply + one merged page

> **Big refactor.** Supersedes the blind "aplicar a todos"/"Aplicar todas" flow, makes tags a
> first-class part of merchant memory, and merges the Memory, Categories and Tags pages into a
> single page. Touches: `services/memory.rs`, `routes/memory.rs`, `routes/items.rs` (apply-memory),
> `routes/ai_tags.rs` (feed memory on apply), `services/ai.rs` (prompt), nav + 3 frontend pages.
> `merchant_memory.tags` already exists (0002) but is dead: never written, not in the model/API/prompt.

### 1. Tags become part of memory

- [ ] `record_confirmation` (called from item confirm/update/bulk) also records the item's normalized
      tags into `merchant_memory.tags`. **Decision: accumulate (union)** — memory tags grow over time;
      a later confirmation with fewer tags doesn't erase earlier ones. Category stays "latest wins".
- [ ] `MemoryEntry` model + `MEMORY_COLS`/`MEMORY_RETURNING` include `tags`;
      `create_memory`/`update_memory` accept `tags` (manual edit = replace).
- [ ] AI extraction prompt (`build_extraction_system_prompt`): memory lines become
      `- merchant → category [tag1, tag2]` (tags only when present), still gated `confirm_count >= 2`.
- [ ] Applying AI tag suggestions (`ai_tags::apply_suggestion`) also feeds merchant memory — today
      it applies tags to the item but never records them, so memory never learns them.
- [ ] Tag rename/merge/delete already cascade to `merchant_memory.tags` (M10) — verify with tests, no code change.

### 2. Apply covers category **and** tags — preview first, then select

- [ ] **Semantics**: category apply keeps replacing/clearing as today; tags are **added** (union),
      never removed — situational tags stay. Per-item `apply_memory` (Lista/Review button) applies both.
- [ ] New `POST /memory/preview` — payload `{ merchant }` or `{ merchant: "all" }`. Returns the
      candidate items that would *actually change* (missing category **or** missing ≥1 memory tag):
      per item `{ item_id, merchant, description, occurred_on, amount_cents, current_category,
      proposed_category, tags_to_add, changes: [category|tags] }`.
- [ ] New `POST /memory/apply` — payload `{ merchant?, ids: [...] }`; applies the remembered
      category + tags **only** to the selected ids (idempotent, transactional).
      Old `apply-all` / `apply-all-global` endpoints are **removed** — preview is the only path.
- [ ] UI: "aplicar" (per memory row) and "Aplicar todas" open an **in-page preview panel** (same
      page, no navigation): table of affected items with checkboxes + select-all, per-row
      "current → proposed" diff (category change / tags to add highlighted), then "Aplicar
      selecionados". Empty preview → "nada a aplicar".

### 3. Merge Memory + Categories + Tags into one page

- [ ] Keep `/memory` as the single route; remove `/categories` and `/tags` from nav + `App.tsx`
      (old URLs redirect to `/memory`).
- [ ] Page layout (tabs or stacked sections, same route):
  - **Memória** — merchant → category + tag chips, inline edit of both, "aplicar" with preview (§2).
  - **Categorias** — create/rename/merge/delete, color/icon, sub-category tree (`parent_id`), usage
    counts. Folds in the remaining "#4 structure & tooling" items (they land for free here).
  - **Tags** — usage counts, rename/merge/delete (already cascades to items/recurring/memory).
- [ ] Layout nav: single "Memória" entry.

### Decisions to confirm before coding

- Tag accumulation vs replace on re-confirm (recommend accumulate/union — multi-valued by nature).
- Remove blind apply endpoints entirely vs keep `apply-all-global` as a no-preview shortcut
  (recommend remove: select-all in the preview covers the same case).
- Keep route `/memory` ("Memória") vs rename to `/learning` ("Regras") — recommend keep `/memory`.

---

## Categories & tags — remaining (#4 structure & tooling)

- Use the `categories.parent_id` tree in the UI (sub-categories; pie rolls up to parent).
- Category merge/rename tools + usage counts.
- Optional tag colors / color presets + emoji icon for categories.

> Done already: tag normalization, seeded categories, CSV bank-category mapping (#1),
> tag autocomplete + tags on rows + tag filter (#2), smarter matching + AI "new category"
> suggestion + one-click memory apply (#3), **tag management page** — usage counts, rename
> (cascades to items/recurring/memory), merge, delete (M9 ✓). Note: memory auto-applies only
> *category* (tags are situational); recurring rules carry category and are the last milestone (M7 ✓).

## Loose ends (deferred, non-blocking)

| # | Item | Notes |
|---|------|-------|
| 1 | **Installment expansion** | Auto-create future installments when `installment_count > 1` (same day-of-month, +1 month each). |
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
