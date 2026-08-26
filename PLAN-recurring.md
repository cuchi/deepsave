# Plan — Recorrentes revamp (v0.9)

> Design doc for the recurring-items feature revamp. Complements `PLAN.md` (the repo-wide
> source of truth). Status: **implemented** (all steps in §7 done; deviations noted inline).

## 1. Core concepts

- **Recurring rules are not real items.** They are pure forecast/analytics constructs
  (this is already true today — they never auto-create items).
- **Rules link to real items** via `items.recurring_id` (schema + model already have the
  column, but **nothing ever writes it today**). Once linked, we can analyze a rule's
  real spend separately and show its true occurrence history.
- **The rule has a `name`** — a free-form label (can be anything: "Netflix", "Minha
  academia", "IPVA"). The name **plays no role in matching**.
- **Matching is done purely through *alias* entries.** Real-world names drift
  (`NETFLIX.COM`, `NETFLIX BR`, `netflix` …), so each rule carries a list of names it
  matches. Aliases always match **exactly** (after normalization: trim + lowercase +
  strip accents), never substring/fuzzy.
- **Names on a rule come in two types** (per-entry, chosen with a checkbox):
  1. **Alias (auto)** — normalized exact match → items link automatically. Default.
  2. **Isolated case (manual)** — one-shot: links the *existing* matching items at save
     time, but future items with that name are **not** auto-matched.
- **Manual linking is a first-class feature**: the user can link any item to any rule by
  hand. This is what makes vague rules viable: a rule named "PAGAMENTO VIA PIX" needs
  **zero** name entries — it simply exists, and the user links items to it manually.
- **Rule tags are derived, not stored.** The rule has no tags of its own: its displayed
  tags are the union of its linked items' tags. If those items disagree on tags (more
  than one distinct non-empty tag set), the card shows a divergence warning.
- **Alias integrity rules**:
  1. A name entry (both types) must **exist in the data** (a real merchant/description
     already seen in items or merchant memory) before it can be added.
  2. **Two rules cannot share the same alias** (auto type). Isolated-case names are
     per-rule manual references and may repeat across rules.

## 2. Data model

Migration `0010_recurring_aliases.sql`:
- `ALTER TABLE recurring_rules RENAME COLUMN description TO name;` — `name` is the
  free-form label (NOT NULL stays).
- `ALTER TABLE recurring_rules DROP COLUMN merchant;` — no longer used by matching and
  referenced nowhere (nothing wrote `items.recurring_id` before). Legacy rules keep their
  old label in `name` and start with **no name entries** → they match nothing until the
  user adds aliases or links items manually. Honest default.
- **New table** `recurring_aliases` (per-entry metadata no longer fits a `text[]`):
  ```sql
  CREATE TABLE recurring_aliases (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id    uuid NOT NULL REFERENCES recurring_rules(id) ON DELETE CASCADE,
    name       text NOT NULL,                        -- stored normalized
    is_alias   boolean NOT NULL DEFAULT true,        -- true = auto-match, false = isolated case
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (rule_id, name)
  );
  CREATE UNIQUE INDEX recurring_aliases_alias_uniq
    ON recurring_aliases (name) WHERE is_alias;      -- DB-level: aliases unique across rules
  ```
- **New column on items**: `linked_manually boolean NOT NULL DEFAULT false` — marks
  user-made links so auto-relink never destroys them (see §3).
- Tags column on `recurring_rules` no longer exists — **dropped in migration 0010**
  (rule tags are derived from linked items, §3). This also removes the tags cascade in
  `services/tags.rs` (rename/merge/delete no longer touch `recurring_rules`) and the
  `recurring_updated` counters in `routes/tags.rs` + the `TagRenameResult` field.

## 3. Matching, linking & manual links

- **Normalization** for all comparisons: trim + lowercase + strip accents
  (`tags::strip_accents` — already exists). Applied to name entries at write time and to
  item text at match time.
- **Match target**: item `merchant`; **only if** the item has no merchant, fall back to
  `description`. Never combined.
- **Match mode**: exact normalized equality against any **alias** (auto) entry.
  Isolated-case names and the rule `name` never participate in auto-matching.
- **Guarantee — at most one rule per item**: alias uniqueness across rules + a single
  match target per item ⇒ two rules can never both auto-match the same item.
- **Exclusions**: do not link installment series (`installment_count > 1`), receipts
  (`parent_id IS NOT NULL`), or non-confirmed items.
- **Manual links win**: an item linked by the user (`linked_manually = true`) is never
  re-linked or unlinked by automation.
- **Rule tags (derived)**: the rule's displayed tags are the union of its linked
  confirmed items' tags. `tags_conflict = true` when those items carry more than one
  distinct non-empty tag set (e.g. one item `[streaming]`, another `[streaming, assinatura]`).
- **When linking runs**:
  1. **On item confirm** — `POST /items/{id}/confirm` hooks `link_item(pool, item_id)`.
  2. **On ingest** — parser-inserted items are already `confirmed`, so the ingest path
     also calls `link_item` for each inserted item (or a single bulk pass per document).
  3. **On rule change** — create/update/delete re-runs reconciliation for that rule:
     `relink_rule` unlinks and re-links only `linked_manually = false` items. When the
     rule gains a new **isolated case** entry, its one-shot link happens here: existing
     matching items get linked with `linked_manually = true`.
  4. **Manual linking** — user picks a rule for one (or many) items: set
     `recurring_id = <rule>`, `linked_manually = true`. Unlink: `recurring_id = NULL`,
     `linked_manually = false`.
  5. **On demand (optional)** — `POST /recurring/reconcile` full pass (idempotent,
     respects manual links).

## 4. Backend changes

### 4.1 Rule API (`routes/recurring.rs`, `models.rs`)
- `RecurringRule` model: `description` → `name`; drop `merchant`; add
  `aliases: Vec<String>` (auto) and `isolated_cases: Vec<String>` (manual) — read from
  `recurring_aliases` (split by `is_alias`). `tags` is **not** a stored field: the
  response carries derived `tags: Vec<String>` + `tags_conflict: bool` (see §3).
- `RecurringInput`: `name`, `aliases?: Vec<String>`, `isolated_cases?: Vec<String>`
  (no `tags`).
- `RecurringRule` gains `days_until?: i64` from the list endpoint.

### 4.2 Next date — never in the past
- New helper in `services/recurring.rs`:
  `advance_next_due(next_due_on, frequency, interval, today) -> NaiveDate`
  — advances by 7d / N months / N years until `>= today`. Pure, idempotent.
- `list` returns the computed `next_due_on` + `days_until` per rule (read-time, no
  write-on-read). Create/update also clamp `next_due_on` to `>= today` as a safety net.
- **Remove** `GET /recurring/upcoming` (folded into `list`).

### 4.3 Linking (`services/recurring.rs`)
- `link_item(pool, item_id)` — auto-match one item against alias entries of all rules;
  skips items with `linked_manually = true`.
- `relink_rule(pool, rule_id)` — unlink + re-link `linked_manually = false` items for one
  rule (handles alias add/remove and isolated-case one-shot linking).
- `reconcile(pool)` — optional full pass.
- Shared matcher: `alias_matches_rule(rule, text) -> bool` (normalized exact equality
  against auto aliases only).

### 4.4 Name-entry validation (in create + update, before persisting)
- Normalize each entry; dedupe within the rule.
- **Must exist in the data** (both types): for each name, check
  `EXISTS (SELECT 1 FROM items WHERE merchant = $1 OR description = $1)`
  (Rust-side normalized comparison first, then the query). Same predicate matching uses
  (§3) — descriptions are included because matching falls back to them for merchant-less
  items (e.g. "PAGAMENTO VIA PIX"). `merchant_memory` needs no separate check: memory
  merchants are derived from items, so they're already covered (kept only as a fallback
  if we ever want to allow aliasing a merchant whose items were all deleted).
- **Global uniqueness** (auto aliases only): enforced by the partial unique index
  (surface as `400` with a friendly message on conflict) + pre-checked for a clear error.
- On failure → `400` with a pt-BR message listing the offending names and the reason
  (e.g. `"alias 'IFOOD' não existe nos dados"` / `"alias 'NETFLIX' já usado pela regra 'X'"`).

### 4.5 Manual linking endpoints (`routes/items.rs`)
- `POST /items/{id}/link-recurring` — body `{ rule_id: uuid | null }`; sets
  `recurring_id` + `linked_manually` accordingly (null = unlink).
- (Optional) `POST /items/link-recurring` — bulk: `{ ids: uuid[], rule_id: uuid | null }`
  for linking many PIX-like items at once.

### 4.6 New endpoints (`routes/recurring.rs`)
- `GET /recurring/merchants?q=` — distinct merchant names (`items.merchant` +
  `merchant_memory.merchant`), `ILIKE '%q%'`, ordered by frequency, capped (e.g. 25).
  Used by the **name** autocomplete and the **name-entry** editor.
- `GET /recurring/merchant-profile?name=` — auto-derivation payload for the add flow
  (only meaningful when the chosen name matches an existing merchant):
  ```json
  { "merchant": "NETFLIX", "amount_cents": -3990, "category_id": "...",
    "category_name": "Streaming",
    "last_occurred_on": "2025-06-10", "suggested_frequency": "monthly" }
  ```
  Source: latest confirmed item for that merchant + `merchant_memory` row.
  `suggested_frequency` uses a standalone `classify_gap(days) -> Option<(frequency, interval)>`
  helper (median day-gap logic — kept when the detection feature was removed; the
  add-flow window suggestion reuses it).
- `GET /recurring/{id}/occurrences` — **query by `items.recurring_id = $1`** (the link is
  the source of truth — manual and auto links alike), `status = 'confirmed'`,
  `ORDER BY occurred_on DESC LIMIT 10`. No name-match fallback: an empty link list shows
  "Nenhuma ocorrência" + a hint ("adicione aliases ou vincule itens manualmente").
- `POST /recurring/reconcile` (optional) — full re-link pass.

### 4.7 Remove the suggestion feature (deferred — will be reimplemented later)
- **Remove** `services::recurring::suggest()` and the `GET /recurring/suggestions` route
  (plus `recurringApi.suggestions` on the frontend and the Sugestões UI section).
- **Keep** `classify_gap()` — extracted for the add-flow window suggestion.
- Detection heuristics stay in git history for the future reimplementation.

### 4.8 Derived rule tags (`routes/recurring.rs` / `services/recurring.rs`)
- In `list`, one query aggregates linked confirmed items' tags per rule:
  `SELECT i.recurring_id, i.tags FROM items i WHERE i.recurring_id IS NOT NULL AND i.status = 'confirmed'`
  → Rust builds the per-rule union (`tags`) and the conflict flag (`tags_conflict`: true
  when ≥ 2 distinct non-empty tag sets among that rule's items).
- Tag renames/merges/deletes already cascade to items (§tags routes) → the derived
  display updates automatically; no rule-side cascade needed.

## 5. Frontend changes

### 5.1 Types / API client (`client.ts`, `lib/types.ts`)
- `RecurringRule`: `name`, `aliases: string[]`, `isolated_cases: string[]`,
  derived `tags: string[]` + `tags_conflict: boolean`, `days_until?: number`
  (drop `merchant`/`description`; tags are not stored).
- `RecurringInput`: `name`, `aliases?`, `isolated_cases?` (no `tags`).
- `TagRenameResult`: drop `recurring_updated` (rule tags are derived now).
- Remove `UpcomingOccurrence` + `recurringApi.upcoming` + `recurringApi.suggestions`;
  add `merchants(q)`, `merchantProfile(name)`, `occurrences(id)`, (optional) `reconcile()`.
- `itemsApi`: add `linkRecurring(id, ruleId | null)`, (optional) `bulkLinkRecurring(ids, ruleId | null)`.

### 5.2 Page layout — 1 section (`pages/Recurring.tsx`)
- **Recorrentes** — the single list of rules with inline next dates (replaces the old
  "Regras" + "Próximas ocorrências" + "Sugestões" sections).

### 5.3 RuleCard (shared by list, edit, add-preview)
- **Header**: Name (`rule.name`) · category chip · **derived tags** chips (union of
  linked items' tags — not editable here, edit tags on the items) · value (`fmtCents`) ·
  window label ("Semanal"/"Mensal"/"Anual", "+a cada N" if interval > 1). When
  `tags_conflict`, an amber ⚠ chip next to the tags, tooltip "Itens vinculados têm
  tags divergentes".
- **Sub-row**: next date with relative label ("em 12d") — never in the past; "—" when
  null; inactive rules dimmed with "pausada" badge. Small counts of name entries
  (e.g. `↻ 2 aliases · 1 caso isolado`) — full names editable in edit mode.
- **Actions**: edit / delete (small text buttons). Edit mode: inline inputs for name,
  category (select), **name entries** (chip editor — see below), value, window, next date;
  **salvar / cancelar**. Saving triggers backend re-link (occurrences update immediately).
  (No tags input — tags always come from the linked items.)
- **Name-entry editor** (replaces the plain "aliases" input):
  - Chip editor with autocomplete from `/recurring/merchants`.
  - A **checkbox "usar como alias"** above the input decides the type of the entry being
    added: checked = **alias** (auto-match, default); unchecked = **caso isolado**
    (one-shot). Existing chips show their type (e.g. alias chips `↻ nome`, isolated chips
    `¹ nome`).
  - Inline validation messages per chip: "não existe nos dados" / "já usado por outra regra".
- **Expandable "Ocorrências recentes"**: chevron toggles a lazy-fetched list from
  `GET /recurring/{id}/occurrences` (date · description · amount). Empty state:
  "Nenhuma ocorrência" + hint. Auto-linked and manually-linked items both appear here.

### 5.4 Add flow
- "Nova regra" panel: **name input with autocomplete** (`/recurring/merchants` — a
  convenience, the name is free text) + **window select**.
- If the chosen name matches an existing merchant: auto-fill value/category, compute
  next date from `last_occurred_on` + chosen window, and **seed that merchant as an
  alias entry** (trivially passes the "exists in data" validation).
- If it doesn't match any merchant: fields default to empty (value —, no category,
  next date —). The rule can still be saved with **zero** name entries — it will be
  populated via manual linking (vague-description case, e.g. "PAGAMENTO VIA PIX").
  The preview stays read-only except name/window (strict add flow — decided); value,
  category and next date are completed afterwards in edit mode.
- Tags are **not** part of the add form: the preview card shows no tags (nothing is
  linked yet); tags appear once items get linked.
- On window change → recompute next date (from last occurrence if known; else "—").
- Render the **same RuleCard in preview mode** with draft data + "Salvar"/"Cancelar" —
  exactly what will appear in the list. Save → `POST /recurring` → backend validates
  name entries + links existing matching items (aliases auto, isolated cases one-shot).

### 5.5 Manual linking (`pages/Lista.tsx`)
- The item ⋯ menu gains **"Vincular a regra…"**: opens a small searchable popover of
  rules (with "Sem regra" to unlink). Selecting one calls
  `itemsApi.linkRecurring(id, ruleId)`; invalidates `items` + `recurring` queries.
  Hidden/renamed to "Desvincular da regra" when the item is already linked.
- The recurring chip (§5.6) doubles as the unlink affordance (optional).
- (Optional) Bulk: the selection bar gains "Vincular a regra…" → same popover →
  `bulkLinkRecurring(ids, ruleId)`.

### 5.6 Main items list — recurring indicator (`pages/Lista.tsx`)
- Items linked to a rule (`items.recurring_id` — already returned by the API) get a
  small tag-like chip in the row, next to the other badges (installment `x/y`, kind,
  category, tags).
- Chip: `↻ recorrente`, same shape as tags but with a distinct accent (e.g.
  `bg-sky-950 text-sky-300` or a bordered variant) so it reads as a link indicator, not a
  tag. `title="Vinculado a uma regra recorrente"`.
- Optional: clicking the chip navigates to `/recurring`.
- Not shown on receipt children rows (root items only).

## 6. Open questions

### 6.1 Need a decision

(none open — see 6.2)

### 6.2 Already decided (documented behavior)

- **Auto-match is exact & normalized** — trim + lowercase + strip accents, never substring/fuzzy.
- **At most one rule per item** — alias uniqueness + single match target make conflicts impossible for auto-links.
- **Rule `name` is a label only** — never participates in matching.
- **Name-entry validation = the matching predicate** — a name must exist as an item
  merchant or description (normalized) before it can be added to a rule.
- **Rule tags are derived from linked items** — never stored on the rule; divergent
  item tags surface as an amber ⚠ warning on the card.
- **Rules may have zero name entries** — viable via manual linking (vague-description case).
- **Next date is never in the past** — computed at read time; also clamped on write.
- **`recurring_id` is the source of truth for occurrences** — no name-match fallback in the occurrences list.
- **Pause/activate** — small inline toggle on the card, outside edit mode.
- **Unknown-name add flow: strict (option a, decided)** — the add preview is read-only
  except name/window. When the name matches no merchant, value/category/next date save
  empty and are completed later in edit mode.
- **`recurring_rules.merchant` is dropped** in migration 0010 — unused, nothing references
  it (decided: proposed default).
- **Isolated-case names may repeat across rules** — only auto aliases are globally unique
  (partial unique index on `is_alias`) (decided: proposed default).
- **Manual links win over auto-match** — auto logic skips `linked_manually = true` items;
  no user-facing prompt (decided: proposed default).

### 6.3 Out of scope (may come back later)

- **Auto-suggested name entries** — from the detection heuristics (deferred together with the suggestion feature).
- **Alias changes cascading to merchant memory** — memory stays keyed by exact merchant name; alias-only items keep their own memory entries.
- **Per-alias advanced metadata** (exact/substring per entry, match priority) — the table supports it later, `text[]` didn't.

## 7. Implementation order

1. Migration 0010 (`name` rename, drop `merchant` + `tags` columns, `recurring_aliases`
   table + partial unique index, `items.linked_manually`).
2. Backend: model/route `name` + `aliases`/`isolated_cases`, derived tags aggregation
   in `list` (`tags` + `tags_conflict`), `advance_next_due` → drop `upcoming`.
3. Backend: remove recurring tags cascade from `services/tags.rs` + `routes/tags.rs`
   (`recurring_updated` out of `TagRename`/responses) + drop field from `TagRenameResult`.
4. Backend: name-entry validation (exists-in-data + alias uniqueness) in create/update.
5. Backend: linking (`link_item`, `relink_rule` with `linked_manually` handling, hooks in
   confirm/ingest + rule create/update/delete, isolated-case one-shot).
6. Backend: manual-link endpoints (`/items/{id}/link-recurring`, optional bulk);
   merchants / merchant-profile / occurrences endpoints; remove `/recurring/suggestions`
   + `suggest()` (keep `classify_gap`).
7. Frontend: types/client → RuleCard → single rules list + expandable occurrences →
   add flow with preview → name-entry editor with checkbox + validation → manual link
   in Lista ⋯ menu → recurring chip → remove old sections (incl. Sugestões).
8. Verify: `cargo build`, `npm run build`, `./scripts/seed.sh`, manual pass.
