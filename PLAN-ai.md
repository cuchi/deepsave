# Plan — AI features (v1.0)

> Design doc for the AI-assisted features on top of the Pluggy-first data model.
> Complements `PLAN.md` (remaining work) and `AGENTS.md` (conventions). Status:
> **proposal** — not yet implemented.

## 0. Why now / what we have

The data is finally **clean and structured** (Pluggy items, categories/tags, merchant
memory, recurring rules, refund links, purchase series). The old document-extraction AI
is gone. That frees the AI budget for what it's actually good at: **judgment, not
parsing**.

Current AI surface (all works except the review UI):
- **AI tagging** (`services/ai_tags.rs`): batch → DeepSeek proposes tags → suggestions
  → apply/dismiss. The Revisar page that surfaced them was deleted in the decommission —
  the flow is broken, but the machinery is intact and reusable.
- **Merchant memory** (category + tags, `confirm_count ≥ 2` gating) — the few-shot
  foundation for every feature below.
- `AiClient` (chat JSON + vision), `ai_calls` token/cost accounting, background worker
  pattern, `memory preview → apply` UX precedent.

## 1. Design principles

1. **AI proposes, never mutates.** Every feature produces a *suggestion* the user
   reviews and applies (inline in the list, not a separate page). Reuses the
   `ai_tag_suggestions` pattern (batch row + suggestion rows + apply/dismiss + memory
   feedback on apply).
2. **Deterministic plumbing, AI at the edges.** Rules handle what they handle well
   (dedup, series, refund linking, recurring aliases). AI fills the gaps: semantic
   matching, classification, narrative.
3. **Compact payloads, cost-conscious.** Batches capped (200 items), per-merchant
   examples capped (3), stable system prefixes for prompt caching, every call recorded
   in `ai_calls` (already the norm). pt-BR prompts.
4. **Memory is the feedback loop.** Applying a proposal feeds `merchant_memory`
   (category + tags) so the next batch gets smarter and the app gets less manual.
5. **On-demand + scheduled.** Buttons for batch features; a monthly digest can run on
   demand (and later on a timer).
6. **The user teaches, the AI executes.** Tag descriptions (F0) and merchant memory
   are the vocabulary the AI is told to use — every prompt injects them.

## 2. Common architecture

```
[enqueue] → ai_proposals (kind, status, created_at)
         → worker: build compact payload → DeepSeek JSON → fill suggestions
         → UI: inline review (apply/dismiss/apply-all)
         → apply: mutate + update merchant_memory
```

- One new table `ai_proposals` generalizes `ai_tag_batches` with a `kind` column
  (`tags | categorize | merchant | refund_link | anomaly | digest`); the existing
  `ai_tag_suggestions` table gains `kind` too (or a parallel `ai_proposal_items`).
  Plus the `tags` registry from F0. Migration 0018.
- `services/ai_features.rs`: per-kind prompt builders + a shared worker
  (`run_worker` mirrors `ai_tags::run_worker`).
- Each kind defines its own **apply** semantics (see below). Applying always updates
  `merchant_memory` when a category/tag is involved.

## 3. Features (ranked)

### Tier 1 — fix + quick wins

#### F0. Tag descriptions — teach the AI (enabler)
- A small **tags registry** table so every tag can carry a description the user
  writes: `CREATE TABLE tags (name text PRIMARY KEY, description text NOT NULL DEFAULT '', created_at timestamptz NOT NULL DEFAULT now())`.
  (Future-proof: can later gain `color` — the long-deferred tag-color loose end.)
- **Registry stays in sync** with the free-form tags on items/memory: rows are
  upserted lazily when a new tag appears, and the existing rename/merge/delete
  cascade (`services/tags.rs`) also updates the registry (rename carries the
  description; merge unions into the target; delete removes the row).
- **UI**: in the Memória page's Tags tab, each tag gets a description field ("o que
  esse tag significa pra mim?"); hover on a tag chip in the list shows the
  description.
- **Prompt injection (the point)**: every AI feature includes the descriptions of
  the tags in scope — the tagging/categorization prompts send `tag 'viagem' = "gastos
  de viagem a trabalho"`, so the AI uses the user's vocabulary instead of guessing.
  This is how the user "teaches" the assistant without writing rules.
- Effort: small (schema + one endpoint + prompt builder change). Enables F1/F2 to be
  much smarter from day one.

#### F1. Rework AI tagging inline (fixes the broken flow)
- Move suggestion review from the (deleted) Revisar page **into Lista**: a pending
  suggestions bar ("N sugestões de tags — Ver/Aplicar todas") + per-row badge with
  apply/dismiss. No new page.
- Keep the current batch/worker/apply code; only the surface changes.
- Effort: small. Unblocks everything else (memory feedback depends on applies).

#### F2. Smart categorization (the big one)
- Target: the **uncategorized items** (Pluggy imports leave ~600 without a mapped
  category) + any item with no category.
- Prompt: item (description, merchant, amount, date, installment) + memory few-shots
  for that merchant + the active category list (name + parent). Output: category or
  `"nova: <sugestão>"` or `null` (unclassifiable — transfers etc.).
- Proposal per item; apply sets `category_id`, creates the category when "nova",
  updates merchant memory (gated ≥ 2 confirmations like today).
- Batch by merchant to keep payloads small and examples relevant.
- Effort: medium (reuses tagging machinery + categories CRUD already exists).
- Value: the single biggest UX win — the manual categorize grind disappears.

#### F3. Monthly digest (delight, cheap)
- On-demand button on Gráficos: aggregate the month (totals, by-category, top
  merchants, new merchants, new subscriptions, upcoming obligations, anomalies) →
  one DeepSeek call → pt-BR narrative paragraph(s) rendered as a card.
- No new tables (computed from existing dashboard queries; store last digest in the
  browser or a small `ai_digests` table).
- Effort: small. High perceived value, near-zero risk.

### Tier 2 — medium effort, real payoff

#### F4. Merchant normalization
- We hit this during dedup: "EMERSONGOSSDACRUZ LAGES BRA" vs "EmersonGossDaCruz".
  AI proposes a **canonical merchant name** per cluster (same tokens/amount range) —
  e.g. renames the merchant on a group of items (and offers to seed a recurring
  alias / memory key).
- Proposal: cluster → suggested canonical name → apply renames `items.merchant`
  (and offers merge into memory/recurring). Non-destructive (one-click per cluster).
- Effort: medium. Value: cleaner list, better memory keys, fewer "duplicates" forever.

#### F5. Refund-counterpart finder (semantic)
- The token matcher leaves refunds unlinked when descriptions differ (LOCALIZA
  +201.47). AI gets the refund + candidate charges (same account, ±90 days, amount
  range) and proposes the link.
- Proposal → apply sets `refunded_item_id` (existing netting kicks in).
- Effort: small-medium. Closes a known gap; few items but the pattern recurs.

#### F6. Anomaly / unexpected-spend alerts
- Rule-based baseline first: per merchant and per category, compare current window vs
  history (mean/std or median; flag > 2× typical or new merchants > threshold).
- AI writes the narrative for flagged items: "IFOOD: R$ 340 este mês vs média R$ 120
  — 4 compras novas" + optional action ("vincular a recorrente?").
- On-demand button ("Analisar anomalias") + optionally after each sync.
- Effort: medium (baseline SQL + one AI call for the narrative).

### Tier 3 — explore later

#### F7. Natural-language assistant
- "quanto gastei em restaurantes em junho?" → DeepSeek maps to the **existing filter
  params** (month, category_ids, tags, kind, date range) as JSON → the app runs the
  query and shows the list/graphs. Structured output keeps it deterministic; the chat
  is a small input in Gráficos/Lista.
- Effort: medium-high (endpoint + UI + follow-up handling). High wow, guard against
  over-scope.

#### F8. Subscription price tracking
- Detect price deltas on recurring rules / merchant history ("Spotify 21,90 → 26,90"),
  flag as a proposal/alert. Rule-based delta + AI confirmation of "same plan".
- Effort: medium. Nice for the forecast/Previsão page.

#### F9. Essential × discretionary classification
- Per-category or per-merchant labels (essential/discretionary) → 50/30/20-style
  breakdown + narrative. AI classifies the category set once (small call); the app
  computes the split from items.
- Effort: small-medium. Cool on Previsão.

## 4. Out of scope (not now)

- Document/OCR/receipt extraction (decommissioned).
- Investment advice, budgeting engines, multi-user anything.
- Auto-mutation without review (everything stays proposal-based).

## 5. Suggested order

1. **F1** (fix inline review) — unblocks applies + memory feedback.
2. **F2** (categorization) — biggest practical win; burns down the uncategorized pile.
3. **F3** (monthly digest) — cheap delight.
4. **F5** (refund finder) + **F6** (anomalies) — close data gaps.
5. **F4** (merchant normalization) — polish.
6. **F7–F9** — explore after the pipeline is proven.

## 6. Tests & accounting

- Each feature: unit tests for the prompt builder (payload shape) + an integration
  test with wiremock for the apply path (like `tests/ai_tags.rs`).
- All calls land in `ai_calls` (purpose = feature kind) — cost stays auditable.
- Keep prompts pt-BR and stable (cacheable).
