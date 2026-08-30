# Plan — AI features (v2, reframed)

> Design doc for the AI-assisted features on top of the Pluggy-first data model.
> Status: F0, F1, F2, F3, F10 implemented. This revision **refocuses the AI on
> judgment** — see principle 2. F5 (refund AI) was dropped as a bad trade.

## 0. Why now / what we have

The data is clean and structured (Pluggy items, categories/tags, change log, recurring
rules, refund links, series). The old document-extraction AI is gone. Current AI
surface: inline tagging (F1), categorization (F2), monthly digest (F3), and the change
log (F10) feeding `hist` into every prompt — plus `mcc`/`pluggy_category`/
`operation_type` metadata and tag descriptions (F0).

## 1. Design principles

1. **AI proposes, never mutates.** Every feature produces a *suggestion* the user
   reviews and applies inline. Reuses the batch → suggestion → apply/dismiss pattern.
2. **The SQL+template test — the reframing.** If a SQL query plus a template can
   produce it, **don't spend an AI call on it**. AI earns its keep only where it
   brings something rules can't:
   - **world knowledge** (merchant → category, "what is this thing"),
   - **fuzzy disambiguation** (same string = two different entities: a restaurant vs
     a person; `IFD*iFood` ≡ `IFOOD.COM`),
   - **interpretation with judgment** (not "spend +49%" but "one-off medical bill,
     not budget creep"),
   - **semantic consistency** (applying the user's tag meanings across merchants).
   Everything else (aggregation, thresholds, exact matching, formatting) stays in
   rules/templates — cheaper, deterministic, explainable.
3. **Compact payloads, cost-conscious.** Batches capped, examples capped, stable
   system prefixes for prompt caching, every call recorded in `ai_calls`.
4. **The change log is the memory.** `change_log` (F10, `hist` per item) + tagged
   examples (`ex`) are the user's ground truth — no merchant-memory table.
5. **On-demand + scheduled.** Buttons for batch features; the digest can run on a
   timer later.
6. **The user teaches, the AI executes.** Tag descriptions (F0) + `hist` + `ex` are
   the vocabulary every prompt is told to use.

## 2. Common architecture

```
[enqueue] → ai_tag_batches (kind: tags | categorize | merchant | anomaly)
         → worker: compact payload (hist/ex/metadata) → DeepSeek JSON → suggestions
         → UI: inline review (apply/dismiss)
         → apply: mutate + log to change_log
```

- Batch/suggestion tables already carry `kind`; new kinds reuse the same machinery.
- `services/ai_features.rs` (or per-kind modules): prompt builders + apply semantics.
- Every apply writes to `change_log` (the durable memory).

## 3. Features (implemented)

- **F0 — Tag descriptions** ✅ (`tags` registry + prompt injection).
- **F1 — Inline tagging** ✅ (amber chips, banner, "Só itens com sugestão").
- **F2 — Smart categorization** ✅ (world knowledge; MCC rule as the deterministic
  layer beneath it).
- **F3 — Monthly digest** ✅ (cheap add-on; honest note: the *data* is rules — the AI
  only picks the framing. Kept because it's one call/month and pleasant, not because
  it's the AI's best use).
- **F10 — Change log** ✅ (append-only history; `hist` in every prompt; the user's
  curation as trajectory, not snapshot).

## 4. Features (planned — reframed)

### F4. Merchant normalization — only the semantic part

- **Rules (already shipped)**: exact/token/`norm_key` merging handles `DIFATTO` vs
  `DI FATTO`, spacing/case variants.
- **AI (the legitimate part)**: clusters that rules *cannot* unify because the names
  are semantically the same brand — `IFD*iFood` ≡ `IFOOD.COM` ≡ `IFOOD PAGO`, or
  `EMERSON GOSS DA CRUZ` (restaurant) vs the person receiving Pix. AI proposes a
  canonical merchant per cluster (world knowledge) + flags same-string-different-
  entity collisions so the user decides.
- Apply: rename `items.merchant` on the cluster (non-destructive, one click per
  cluster). Feeds `hist` + recurring alias suggestions.
- Effort: medium. Value: genuinely reduces duplicate-merchant noise that rules miss.

### F6. Anomaly alerts — rules detect, AI interprets

- **Rules (the real work)**: per-merchant and per-category baselines (median/std,
  rolling window), flag > 2× typical or unusual frequency; deterministic, cheap.
- **AI (the judgment)**: interprets the flagged set — "one-off medical bill" vs
  "subscription creep" vs "seasonal" — and proposes an action ("vincular a
  recorrente?", "rever assinatura"). The narrative is the *output of judgment*, not
  a formatting step: the model sees the flagged items + history and says what the
  user should *look at*.
- Effort: medium (baseline SQL + one AI call for the interpretation).

### F11. Relationship classification (new — the strongest untapped case)

- Monthly PIX/TED to the **same person** (counterparty CNPJ/CPF — 88% coverage) is a
  *relationship*, not a category: rent, child support, business payouts, family help.
- **Rules**: detect recurring same-counterparty transfers (frequency + stability).
- **AI**: labels the relationship (world knowledge + name/amount patterns) and
  proposes a tag + recurring rule ("aluguel" + monthly rule). Genuine judgment —
  rules can't infer "this is rent".
- Apply: sets tag + links/creates a recurring rule. Feeds `hist`.
- Effort: medium. High value: the recurring/forecast layer gets smarter for free.

### F7. Natural-language assistant (kept — genuinely useful)

- "quanto gastei em restaurantes em junho?" → AI maps to existing filter params as
  JSON (deterministic); the app runs the query. The AI's value is mapping *language*
  to the filter space — rules can't do that.
- Effort: medium-high. Guard against over-scope.

### Dropped

- **F5 (refund-counterpart AI)** — rules already solve the clean cases; the rare
  ambiguous ones are higher-risk with AI (less explainable, wrong links hurt the
  netting). Keep rules + the manual review; no AI call.
- **F8 (subscription price tracking)** — rule-based delta is deterministic and
  explainable; no AI needed (mark as a plain feature if wanted).
- **F9 (essential × discretionary)** — borderline: a *single* AI call to label the
  category set is defensible (world knowledge), then rules compute the split. Only
  build if the user wants the 50/30/20 view; the AI part is one call.

## 5. Out of scope (not now)

- Document/OCR/receipt extraction (decommissioned).
- Investment advice, budgeting engines, multi-user anything.
- Auto-mutation without review (everything stays proposal-based).
- Any feature that fails the SQL+template test.

## 6. Suggested order

1. **F4** (semantic merchant merging) — cleans the data the other features rely on.
2. **F11** (relationship classification) — biggest genuine-judgment win.
3. **F6** (anomaly interpretation) — rules first, AI interpretation second.
4. **F7** (NL assistant) — explore when the pipeline is proven.

## 7. Tests & accounting

- Each feature: unit tests for the prompt builder + a wiremock integration test for
  the apply path (like `tests/ai_tags.rs`).
- All calls land in `ai_calls` (purpose = feature kind).
- Every apply logs to `change_log`.
