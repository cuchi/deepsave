export interface Category {
  id: string
  parent_id: string | null
  name: string
  color: string | null
  icon: string | null
  is_active: boolean
}

export interface Item {
  id: string
  document_id: string | null
  source: string
  kind: string
  status: string
  account_id: string | null
  transfer_group_id: string | null
  installment: number | null
  installment_count: number | null
  recurring_id: string | null
  occurred_on: string
  posted_on: string | null
  merchant: string | null
  description: string
  amount_cents: number
  currency: string
  category_id: string | null
  suggested_category: string | null
  tags: string[]
  raw_line: string | null
  match_confidence: number | null
  created_at: string
  updated_at: string
  /** Bank slug (nubank | caixa | c6) — derived from document source or Pluggy account. */
  bank: string | null
  /** Display label of the source account ("Nubank - Cartão", …). */
  source_label: string | null
  /** Pluggy transaction id — null for legacy document items not yet merged. */
  external_id: string | null
  /** The expense this refund reverses (null when unlinked). */
  refunded_item_id: string | null
}

export interface RecurringRule {
  id: string
  name: string
  amount_cents: number
  currency: string
  category_id: string | null
  category_name: string | null
  frequency: string
  interval: number
  day_of_month: number | null
  next_due_on: string | null
  is_active: boolean
  source: string
  created_at: string
  updated_at: string
  /** Auto-match names (exact, normalized). */
  aliases: string[]
  /** One-shot manual references (no auto-match). */
  isolated_cases: string[]
  /** Derived from linked items (union of their tags). */
  tags: string[]
  /** True when linked items carry divergent tag sets. */
  tags_conflict: boolean
  days_until: number | null
}

export interface RecurringOccurrence {
  occurred_on: string
  description: string
  amount_cents: number
  tags: string[]
  linked_manually: boolean
}

export interface MerchantProfile {
  merchant: string
  amount_cents: number
  category_id: string | null
  category_name: string | null
  last_occurred_on: string
  suggested_frequency: string
  suggested_interval: number
  /** Last occurrence advanced by the suggested window (never in the past). */
  next_due_on: string | null
}

export interface TagUsage {
  tag: string
  count: number
}

export interface ItemSummary {
  count: number
  total_cents: number
}

export interface AiTagBatch {
  id: string
  status: 'pending' | 'processing' | 'done' | 'failed'
  error_message: string | null
  item_count: number
  created_at: string
  processed_at: string | null
  kind: 'tags' | 'categorize'
}

export interface SuggestionDetail {
  id: string
  batch_id: string
  batch_status: string
  batch_kind: 'tags' | 'categorize' | 'full'
  item_id: string
  suggested_tags: string[]
  /** Category proposal for categorize batches ('' when none; may be "nova: X"). */
  suggested_category: string
  status: 'pending' | 'applied' | 'dismissed'
  created_at: string
  merchant: string | null
  description: string
  amount_cents: number
  occurred_on: string
  category_id: string | null
  category_name: string | null
  tags: string[]
  document_id: string | null
  pluggy_category: string | null
  mcc: number | null
  operation_type: string | null
  payment_method: string | null
}

// ---------- Pluggy ----------

export interface PluggyStatus {
  configured: boolean
  auth: 'api_key' | 'client' | 'none'
  items: number
  accounts: number
}

export interface PluggyAccount {
  pluggy_account_id: string
  name: string
  account_type: string | null
  bank: string | null
  last_sync_at: string | null
  item_count: number
  first_date: string | null
  last_date: string | null
}

export interface PluggyAccountSync {
  pluggy_account_id: string
  name: string
  new: number
}

export interface PluggySyncResult {
  configured: number
  accounts: PluggyAccountSync[]
  new: number
}
