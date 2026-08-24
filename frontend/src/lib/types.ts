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
  last_seen: string | null
}

export interface CoverageData {
  months: string[]
  sources: CoverageSource[]
}
