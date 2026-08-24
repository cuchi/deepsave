# DeepSave — Remaining Plan

> v0.7 — only remaining work. Completed work is archived in [`PLAN-DONE.md`](./PLAN-DONE.md).

## M7 — Recurring items (next milestone)

1. **Recurrence detection** — find repeats across confirmed items (same merchant + similar
   amount + regular interval); suggest a `recurring_rules` record **after 2 repeats** and
   **require user confirmation** (no silent auto-create).
2. **`recurring_rules` CRUD** — API + `/recurring` page: list, add/edit, enable/disable.
3. **Upcoming occurrences** — `GET /api/recurring/upcoming`.
4. **Auto-create** the next occurrence as `pending_review` a few days before `next_due_on`
   (default 3 days, configurable).
5. Dashboard **"upcoming recurring"** indicator.

The `recurring_rules` table and the `items.recurring_id` column already exist — only the
logic and UI are missing.

---

## M8 — Sources & coverage (new)

### Concept & naming
The app can't produce a trustworthy picture without complete data from **6 fundamental
sources** (3 banks × 2 products). Suggested naming: the entities are **sources**, the
tracking feature is **coverage** (alternatives: "core sources", "required feeds",
"statement coverage").

The 6 sources:

| # | Bank | Product | Today's file signature |
|---|------|---------|------------------------|
| 1 | Nubank | bank statement | `NU_*.csv` — `Data,Valor,Identificador,Descrição` |
| 2 | Nubank | credit card | `Nubank_*.csv` — `date,title,amount` |
| 3 | C6 | bank statement | `01M0SQKX….csv` — `EXTRATO DE CONTA CORRENTE C6 BANK` *(new)* |
| 4 | C6 | credit card | `Fatura_*.csv` — `Data de Compra;…;Parcela;…` |
| 5 | Caixa | bank statement | `comprovante*.pdf` — "Extrato por período" |
| 6 | Caixa | credit card | `comprovante*.pdf` — "Cartões Caixa" / "COMPRAS (Cartão …)" *(new)* |

### Data model
```sql
sources (
  id           uuid PK,
  bank         text NOT NULL,          -- 'nubank' | 'c6' | 'caixa'
  kind         text NOT NULL,          -- 'bank_statement' | 'credit_card'
  name         text NOT NULL,          -- display name
  enabled      boolean NOT NULL DEFAULT true,
  account_id   uuid NULL REFERENCES accounts(id),
  sort_order   integer NOT NULL DEFAULT 0,
  created_at   timestamptz NOT NULL DEFAULT now()
);
-- seeded with the 6 rows above
```
- `documents.source_id uuid NULL REFERENCES sources(id)` — set at upload time.
- Coverage is **derived**, not stored: per source, the set of months that have ≥1 item
  (bank statements: item months; credit cards: fatura billing month).

### Source detection (bank × kind)
Filename alone is ambiguous (`comprovante*.pdf` is both Caixa products), so detect from
**content + filename**:

- Nubank bank → CSV header `Data,Valor,Identificador,Descrição`
- Nubank card → CSV header `date,title,amount`
- C6 bank → CSV body starts `EXTRATO DE CONTA CORRENTE C6 BANK` / header `Data Lançamento,…Saldo do Dia`
- C6 card → CSV header `Data de Compra;…;Parcela;…`
- Caixa bank → PDF text contains `Extrato por período` + `Conta:`
- Caixa card → PDF text contains `Cartões Caixa` / `COMPRAS (Cartão …)`

Fallback: user can set the source manually on a document if detection fails.

### Coverage UX
- **Dashboard alert**: "N sources missing for this month" (amber), clickable.
- **`/coverage` page**: matrix of the 6 sources × recent months (green = present, red = missing),
  plus per-source "last seen" date.
- Per-source "expected next": end of the current month.

### Resolved decisions
1. **Coverage window**: last **12 months**, stored as a single named constant
   (`COVERAGE_MONTHS`, in `config.rs`, optionally overridable via env) so it's easy to tweak.
2. **Investments**: a new `investment` kind — items are **kept (tracked)** but **excluded
   from all spend/income calculations** (like `card_payment`). Applies to C6 `EMISSAO DE CDB`
   (out), `RESGATE DE CDB` (in), and investment taxes (`IR RES FUNDOS ACOES`).
   Implementation notes: add `investment` to the AI prompt kind enum, to `normalize_kind`,
   and to the frontend kind labels/sign (neutral, no `−`); dashboards/trend/month-view already
   exclude it because they only sum `expense`/`income`.

---

## M9 — New parsers for the two new sources

### C6 bank statement CSV (`01M0SQKX…csv`)
- Starts with a **BOM** and **metadata lines** before the CSV header (skip them).
- CSV header: `Data Lançamento,Data Contábil,Título,Descrição,Entrada(R$),Saída(R$),Saldo do Dia(R$)`
  - dates `DD/MM/YYYY`, amounts dot-decimal, separate **Entrada** (in) / **Saída** (out) columns.
- Kind mapping (Título/Descrição → kind):
  - `PGTO FAT CARTAO` / `Fatura de cartão` → `card_payment`
  - `TRANSF ENVIADA PIX` / `Pix enviado` → `transfer_out`
  - `TRANSF RECEBIDA PIX` / `Pix recebido` → `transfer_in`
  - `DEBITO DE CARTAO` → `expense` (debit-card purchase)
  - `RESGATE DE CDB` → `investment` (redemption, in — tracked, excluded)
  - `EMISSAO DE CDB` → `investment` (out — tracked, excluded)
  - `IR RES FUNDOS ACOES` → `investment` (investment tax — tracked, excluded)

### Caixa credit card fatura PDF (`comprovante2026-08-24_084508.pdf`)
- Billing month from page 1: `VENCIMENTO 15/08/2026`.
- Items live in page 2 under **`COMPRAS (Cartão 5451)`**, table columns:
  `Data | Descrição | Cidade/País | Valor U$$ | Crédito/Débito`, e.g.
  `19/07   IFD*iFood   Osasco   12,90D`
  - date is `DD/MM` (year inferred from VENCIMENTO), `D` = debit (expense), `C` = credit (refund/income).
- Also parse the **`ANUIDADE`** section (annual fee) and, if present, `SAQUES` / `PARCELAMENTO`.
- Layout-aware text extraction (like `pdftotext -layout`) is required to preserve columns.
- Note: the COMPRAS table has **no `Parcela`** column — installment detail for Caixa card is a
  known limitation to revisit.

---

## UI improvements — items list (month view)

- [x] **Bank logo** per item (from `frontend/public/logos/{nubank,c6,caixa}.svg`), resolved via document → source → bank.
- [x] **Date** shown on each row (dd/mm).
- [x] **Category** chip (colored dot + name) on each row.
- [x] Collapse the 3 action buttons (editar / +sub / apagar) into a single **`⋯` menu** (also holds "Detalhes").
- [x] **Negatives in red, positives in green** for amounts.
- [x] **Long descriptions** (e.g. `Transferência enviada pelo Pix - TIM S A - 02.421… - ITAÚ…`) show a short merchant title; the full text is kept in an expandable **details panel**.
- [x] Merchant extraction for Nubank account descriptions (parser stores `items.merchant`; full description preserved).

---

## Categories & tags improvements

**1. Quick wins (high value)**
- Normalize tags on save: trim, lowercase, strip accents, dedup.
- Seed a starter category set (Supermercado, Transporte, Saúde, Moradia, Lazer, Assinaturas…).
- Map CSV bank categories → our tree (e.g. C6 "Supermercados / Mercearia / Padarias" → "Supermercado").

**2. Tag UX**
- Tag autocomplete in the item form (suggest existing tags).
- Show tags as chips on list rows.
- Add a tag filter in the month view (next to the category filter).

**3. Smarter category matching**
- AI "new category" suggestion: surface unknown AI categories in Review as "Nova categoria: X" instead of dropping them.
- Normalize matching (lowercase + strip accents) + alias table.
- One-click apply from `merchant_memory` ("Usual: Supermercado · [mercado]").

**4. Structure & tooling**
- Use the `categories.parent_id` tree in the UI (sub-categories; pie rolls up to parent).
- Tag management page: usage counts, rename (cascades), merge, delete.
- Category merge/rename tools + usage counts.
- Optional tag colors / color presets + emoji icon for categories.

(M7 tie-in: recurring rules carry category + tags, so auto-created occurrences inherit them.)

---

## Loose ends (deferred, non-blocking)

| # | Item | Notes |
|---|------|-------|
| 1 | **Installment expansion** | Auto-create future installments when `installment_count > 1` (same day-of-month, +1 month each). |
| 2 | **Receipt coverage %** indicator | Dashboard metric: how much statement spend is broken down by receipts. |
| 3 | **`/api/usage`** endpoint + UI | AI token/cost aggregation (data already recorded in `ai_calls`). |
| 4 | **Scanned PDF support** | Currently fails with "no text layer"; needs render-pages → vision/OCR. |
| 5 | **CSV bank-category mapping** | Map C6's "Categoria" (and others) into our category tree — currently stored as a *tag*, so CSV items have no `category_id`. |
| 6 | **`deepseek-v4-pro`** | Wire up for hard linking/reconciliation (only flash + vision are used today). |
| 7 | **Document ↔ account** | `documents.account_id` is never set on upload (superseded by `source_id` in M8). |
| 8 | **Amount-range filter** | `GET /api/items` has text/category/kind filters but no `min/max` amount range. |

## Deferred / post-MVP (explicitly out of scope)

- Budgeting (per-category monthly budgets).
- Multi-currency accounting / FX logic.
- Mobile apps (responsive web only).
- Automatic bank API integrations (Open Banking / Plaid-like).
