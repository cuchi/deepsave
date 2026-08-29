import axios from 'axios'
import type {
  AiTagBatch,
  Category,
  Item,
  ItemSummary,
  MerchantProfile,
  PluggyAccount,
  PluggyStatus,
  PluggySyncResult,
  RecurringOccurrence,
  RecurringRule,
  SuggestionDetail,
  TagUsage,
} from '../lib/types'

// `indexes: null` serializes array params as repeated keys (?tags=a&tags=b)
// instead of the default ?tags[]=a&tags[]=b (which the backend can't parse).
const api = axios.create({
  baseURL: '/api',
  withCredentials: true,
  paramsSerializer: { indexes: null },
})

export const authApi = {
  async me(): Promise<{ authenticated: boolean }> {
    return (await api.get<{ authenticated: boolean }>('/auth/me')).data
  },
  async login(password: string): Promise<{ authenticated: boolean }> {
    return (await api.post('/auth/login', { password })).data
  },
  async logout(): Promise<{ ok: boolean }> {
    return (await api.post('/auth/logout')).data
  },
}

export interface CategoryInput {
  name: string
  parent_id: string | null
  color: string | null
  icon: string | null
}

export const categoriesApi = {
  async list(): Promise<Category[]> {
    return (await api.get<Category[]>('/categories')).data
  },
  async create(input: CategoryInput): Promise<Category> {
    return (await api.post<Category>('/categories', input)).data
  },
  async update(
    id: string,
    input: CategoryInput & { is_active: boolean },
  ): Promise<Category> {
    return (await api.patch<Category>(`/categories/${id}`, input)).data
  },
  async remove(id: string): Promise<{ ok: boolean }> {
    return (await api.delete(`/categories/${id}`)).data
  },
}

export interface ItemInput {
  parent_id?: string | null
  kind?: string
  account_id?: string | null
  installment?: number | null
  installment_count?: number | null
  occurred_on: string
  merchant?: string | null
  description: string
  amount_cents: number
  currency?: string
  category_id?: string | null
  tags?: string[]
  /** Feed the categorization memory on this edit (defaults on for single edits). */
  update_memory?: boolean
}

export interface ItemListParams {
  month?: string
  status?: string
  search?: string
  /** Comma-separated category ids (OR). */
  category_ids?: string
  kind?: string
  /** Comma-separated tags (OR: item carries any). */
  tags?: string
  bank?: string
  sort?: string
  limit?: number
  /** 'first_only' keeps non-installments + 1st parcel; 'only' shows just installments. */
  installments?: 'all' | 'first_only' | 'only'
  /** Inclusive date-range bounds (YYYY-MM-DD). */
  date_from?: string
  date_to?: string
}

export interface BulkItemUpdateInput {
  ids: string[]
  kind?: string
  /** undefined = keep, null = clear */
  category_id?: string | null
  tags?: string[]
  tags_mode?: 'replace' | 'add' | 'remove'
  update_memory?: boolean
}

export const itemsApi = {
  async list(params: ItemListParams = {}): Promise<Item[]> {
    return (await api.get<Item[]>('/items', { params })).data
  },
  async listByStatus(status: string): Promise<Item[]> {
    return (await api.get<Item[]>('/items', { params: { status } })).data
  },
  async get(id: string): Promise<Item> {
    return (await api.get<Item>(`/items/${id}`)).data
  },
  async create(input: ItemInput): Promise<Item> {
    return (await api.post<Item>('/items', input)).data
  },
  async update(id: string, input: ItemInput): Promise<Item> {
    return (await api.patch<Item>(`/items/${id}`, input)).data
  },
  async bulkUpdate(input: BulkItemUpdateInput): Promise<{ updated: number }> {
    return (await api.patch<{ updated: number }>('/items/bulk', input)).data
  },
  async summary(params: ItemListParams = {}): Promise<ItemSummary> {
    return (await api.get<ItemSummary>('/items/summary', { params })).data
  },
  async remove(id: string): Promise<{ ok: boolean }> {
    return (await api.delete(`/items/${id}`)).data
  },
  async confirm(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/items/${id}/confirm`)).data
  },
  async reject(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/items/${id}/reject`)).data
  },
  async applyMemory(id: string): Promise<Item> {
    return (await api.post<Item>(`/items/${id}/apply-memory`)).data
  },
  async acceptSuggestion(id: string): Promise<Item> {
    return (await api.post<Item>(`/items/${id}/accept-suggestion`)).data
  },
  /** Link one item to a recurring rule (`ruleId: null` unlinks). */
  async linkRecurring(id: string, ruleId: string | null): Promise<{ ok: boolean }> {
    return (await api.post(`/items/${id}/link-recurring`, { rule_id: ruleId })).data
  },
  /** Link many items to a rule at once (`ruleId: null` unlinks). */
  async bulkLinkRecurring(ids: string[], ruleId: string | null): Promise<{ ok: boolean; updated: number }> {
    return (await api.post('/items/link-recurring', { ids, rule_id: ruleId })).data
  },
}

export interface CategoryTotal {
  category_id: string
  name: string
  color: string | null
  total_cents: number
}

export interface MerchantTotal {
  merchant: string
  total_cents: number
}

export interface DashboardData {
  month: string
  total_spend_cents: number
  total_income_cents: number
  by_category: CategoryTotal[]
  top_merchants: MerchantTotal[]
  pending_count: number
}

export interface TrendPoint {
  month: string
  spend_cents: number
  income_cents: number
}

export interface DashboardParams {
  month?: string
  date_from?: string
  date_to?: string
  search?: string
  /** Comma-separated category ids (OR). */
  category_ids?: string
  kind?: string
  /** Comma-separated tags (OR). */
  tags?: string
  bank?: string
  installments?: 'all' | 'first_only' | 'only'
}

export interface DailyPoint {
  date: string
  key: string | null
  total_cents: number
}

export interface TagTotal {
  tag: string
  total_cents: number
}

export interface MonthlyCost {
  monthly_cents: number
  rule_count: number
}

export interface ExpectedSpend {
  installments_cents: number
  recurring_cents: number
  total_cents: number
}

export interface ForecastPoint {
  month: string
  installments_cents: number
  recurring_cents: number
  total_cents: number
}

export interface UpcomingItem {
  date: string
  kind: 'parcel' | 'recurring'
  description: string
  category_name: string | null
  amount_cents: number
  progress: string | null
}

export const dashboardApi = {
  async get(params: DashboardParams = {}): Promise<DashboardData> {
    return (await api.get<DashboardData>('/dashboard', { params })).data
  },
  async trend(months = 12, params: DashboardParams = {}): Promise<TrendPoint[]> {
    return (await api.get<TrendPoint[]>('/dashboard/trend', {
      params: { months, ...params },
    })).data
  },
  /** Daily expense totals: `stack_by='category'` (stacked bar) or `'none'` (calendar). */
  async daily(params: DashboardParams & { stack_by?: 'category' | 'none' } = {}): Promise<DailyPoint[]> {
    return (await api.get<DailyPoint[]>('/dashboard/daily', { params })).data
  },
  /** Top tags by expense total (spend carrying each tag, overlap allowed). */
  async tags(params: DashboardParams = {}): Promise<TagTotal[]> {
    return (await api.get<TagTotal[]>('/dashboard/tags', { params })).data
  },
  /** Expected spend for a future period (installments + recurring). */
  async expected(params: DashboardParams = {}): Promise<ExpectedSpend> {
    return (await api.get<ExpectedSpend>('/dashboard/expected', { params })).data
  },
  /** Expected spend per month for the next N months. */
  async forecast(months = 3): Promise<ForecastPoint[]> {
    return (await api.get<ForecastPoint[]>('/dashboard/forecast', { params: { months } })).data
  },
  /** Flat, dated feed of the next obligations within `days`. */
  async upcoming(days = 90): Promise<UpcomingItem[]> {
    return (await api.get<UpcomingItem[]>('/dashboard/upcoming', { params: { days } })).data
  },
}

export const aiTagsApi = {
  /** Enqueue AI tagging for the selected items. */
  async createBatch(ids: string[]): Promise<AiTagBatch> {
    return (await api.post<AiTagBatch>('/ai-tags/batches', { ids })).data
  },
  async listBatches(): Promise<AiTagBatch[]> {
    return (await api.get<AiTagBatch[]>('/ai-tags/batches')).data
  },
  async listSuggestions(batchId?: string): Promise<SuggestionDetail[]> {
    return (await api.get<SuggestionDetail[]>('/ai-tags/suggestions', {
      params: batchId ? { batch_id: batchId } : {},
    })).data
  },
  /** Apply a suggestion; `tags` overrides the stored proposal (post-edit). */
  async apply(id: string, tags?: string[]): Promise<{ ok: boolean; tags: string[] }> {
    return (await api.post(`/ai-tags/suggestions/${id}/apply`, tags ? { tags } : {})).data
  },
  async dismiss(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/ai-tags/suggestions/${id}/dismiss`)).data
  },
  async applyAll(batchId?: string): Promise<{ ok: boolean; applied: number }> {
    return (await api.post('/ai-tags/suggestions/apply-all', batchId ? { batch_id: batchId } : {})).data
  },
  async dismissAll(batchId?: string): Promise<{ ok: boolean; dismissed: number }> {
    return (await api.post('/ai-tags/suggestions/dismiss-all', batchId ? { batch_id: batchId } : {})).data
  },
}

export const tagsApi = {
  async list(): Promise<string[]> {
    return (await api.get<string[]>('/tags')).data
  },
  async usage(): Promise<TagUsage[]> {
    return (await api.get<TagUsage[]>('/tags/usage')).data
  },
  async rename(from: string, to: string): Promise<TagRenameResult> {
    return (await api.patch<TagRenameResult>('/tags/rename', { from, to })).data
  },
  async merge(from: string, into: string): Promise<TagRenameResult> {
    return (await api.post<TagRenameResult>('/tags/merge', { from, into })).data
  },
  async remove(tag: string): Promise<TagRenameResult> {
    return (await api.delete<TagRenameResult>(`/tags/${encodeURIComponent(tag)}`)).data
  },
}

export interface TagRenameResult {
  ok: boolean
  items_updated: number
  memory_updated: number
}

export interface MemoryEntry {
  id: string
  merchant: string
  category_id: string | null
  category_name: string | null
  tags: string[]
  confidence: number
  confirm_count: number
  last_confirmed_at: string | null
}

export interface MemoryInput {
  merchant: string
  category_id: string | null
  tags?: string[]
}

export interface MemoryUpdateInput {
  category_id: string | null
  tags?: string[]
}

/** One item the user can select to apply memory to (from `/memory/preview`). */
export interface MemoryPreviewItem {
  item_id: string
  merchant: string
  description: string
  occurred_on: string
  amount_cents: number
  current_category: string | null
  proposed_category: string | null
  tags_to_add: string[]
  /** Subset of ['category', 'tags'] — what would change. */
  changes: string[]
}

export const memoryApi = {
  async list(): Promise<MemoryEntry[]> {
    return (await api.get<MemoryEntry[]>('/memory')).data
  },
  async create(input: MemoryInput): Promise<MemoryEntry> {
    return (await api.post<MemoryEntry>('/memory', input)).data
  },
  async update(id: string, input: MemoryUpdateInput): Promise<MemoryEntry> {
    return (await api.patch<MemoryEntry>(`/memory/${id}`, input)).data
  },
  async remove(id: string): Promise<{ ok: boolean }> {
    return (await api.delete(`/memory/${id}`)).data
  },
  async preview(merchant: string | null): Promise<MemoryPreviewItem[]> {
    return (await api.post<MemoryPreviewItem[]>('/memory/preview', { merchant })).data
  },
  async apply(merchant: string | null, ids: string[]): Promise<{ updated: number }> {
    return (await api.post('/memory/apply', { merchant, ids })).data
  },
}

export interface RecurringInput {
  name: string
  amount_cents: number
  frequency: string
  interval?: number
  day_of_month?: number | null
  next_due_on?: string | null
  is_active?: boolean
  aliases?: string[]
  isolated_cases?: string[]
}

export const recurringApi = {
  async list(): Promise<RecurringRule[]> {
    return (await api.get<RecurringRule[]>('/recurring')).data
  },
  async monthlyCost(): Promise<MonthlyCost> {
    return (await api.get<MonthlyCost>('/recurring/monthly-cost')).data
  },
  async create(input: RecurringInput): Promise<RecurringRule> {
    return (await api.post<RecurringRule>('/recurring', input)).data
  },
  async update(id: string, input: RecurringInput): Promise<RecurringRule> {
    return (await api.patch<RecurringRule>(`/recurring/${id}`, input)).data
  },
  async remove(id: string): Promise<{ ok: boolean }> {
    return (await api.delete(`/recurring/${id}`)).data
  },
  async occurrences(id: string): Promise<RecurringOccurrence[]> {
    return (await api.get<RecurringOccurrence[]>(`/recurring/${id}/occurrences`)).data
  },
  async merchants(q: string): Promise<string[]> {
    return (await api.get<string[]>('/recurring/merchants', { params: { q } })).data
  },
  async merchantProfile(name: string): Promise<MerchantProfile> {
    return (await api.get<MerchantProfile>('/recurring/merchant-profile', { params: { name } })).data
  },
}

export interface TableCount {
  table: string
  count: number
  size_bytes: number
}

export interface StatusCount {
  status: string
  count: number
}

export interface SystemInfo {
  db_size_bytes: number
  storage_size_bytes: number
  storage_file_count: number
  table_counts: TableCount[]
  items_by_status: StatusCount[]
  documents_by_status: StatusCount[]
}

export const systemApi = {
  async get(): Promise<SystemInfo> {
    return (await api.get<SystemInfo>('/system')).data
  },
}


export const banksApi = {
  async list(): Promise<string[]> {
    return (await api.get<string[]>('/banks')).data
  },
}

export const pluggyApi = {
  async status(): Promise<PluggyStatus> {
    return (await api.get<PluggyStatus>('/pluggy/status')).data
  },
  async accounts(): Promise<PluggyAccount[]> {
    return (await api.get<PluggyAccount[]>('/pluggy/accounts')).data
  },
  /** Incremental by default; pass from/to (YYYY-MM-DD) to force a period. */
  async sync(from?: string, to?: string): Promise<PluggySyncResult> {
    return (await api.post<PluggySyncResult>('/pluggy/sync', undefined, {
      params: from || to ? { from, to } : {},
    })).data
  },
}
