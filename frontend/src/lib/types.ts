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
  parent_id: string | null
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
}

export interface DocumentSummary {
  id: string
  kind: string
  filename: string
  content_type: string
  status: string
  error_message: string | null
  uploaded_at: string
  processed_at: string | null
  item_count: number
  source_id: string | null
  first_date: string | null
  last_date: string | null
}

export interface DocumentDetail extends DocumentSummary {
  ocr_text: string | null
  items: Item[]
}

export interface MatchDetail {
  id: string
  parent_item_id: string
  child_item_id: string
  source: string
  confidence: number
  status: string
  parent: Item
  child: Item
}

export interface Source {
  id: string
  bank: string
  kind: string
  name: string
  enabled: boolean
  account_id: string | null
  sort_order: number
  created_at: string
}

export interface CoverageSource {
  id: string
  name: string
  bank: string
  kind: string
  enabled: boolean
  present: string[]
  /** Months only partially covered by bank statements (period doesn't span the full month). */
  partial: string[]
  last_seen: string | null
}

export interface CoverageData {
  months: string[]
  sources: CoverageSource[]
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
}

export interface SuggestionDetail {
  id: string
  batch_id: string
  batch_status: string
  item_id: string
  suggested_tags: string[]
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
}

// ---------- Pluggy ----------

export interface PluggyStatus {
  configured: boolean
  auth: 'api_key' | 'client' | 'none'
  items: number
  accounts: number
}

export interface PluggyConnector {
  id: number
  name: string
  kind: string | null
  oauth: boolean
  mfa: boolean
  open_finance: boolean
  image_url: string | null
  credentials: {
    name: string
    label: string | null
    type: string | null
    optional: boolean
    placeholder: string | null
  }[]
}

export interface PluggyAccount {
  id: string
  pluggy_account_id: string
  account_id: string | null
  name: string
  account_type: string | null
  subtype: string | null
  currency: string
  balance: number | null
  credit_limit: number | null
  due_date: string | null
  close_date: string | null
  last_sync_at: string | null
}

export interface PluggyItem {
  id: string
  pluggy_id: string
  connector_id: number | null
  connector_name: string | null
  status: string
  execution_status: string | null
  error: unknown
  status_detail: string | null
  last_updated_at: string | null
  last_sync_at: string | null
  created_at: string
  item_count: number
  oauth_url: string | null
  accounts: PluggyAccount[]
}

export interface PluggySyncResult {
  status: string
  execution_status?: string | null
  imported: number
  pending?: boolean
  error?: unknown
  status_detail?: string | null
}
