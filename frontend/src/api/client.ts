import axios from 'axios'
import type {
  Category,
  CoverageData,
  DocumentDetail,
  DocumentSummary,
  Item,
  MatchDetail,
  RecurringRule,
  RecurringSuggestion,
  Source,
  UpcomingOccurrence,
} from '../lib/types'

const api = axios.create({ baseURL: '/api', withCredentials: true })

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
}

export interface ItemListParams {
  month?: string
  status?: string
  search?: string
  category_id?: string
  kind?: string
  tag?: string
  bank?: string
  sort?: string
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
}

export type DocumentKind =
  | 'card_statement'
  | 'bank_statement'
  | 'receipt'
  | 'payment_slip'

export const documentsApi = {
  async list(): Promise<DocumentSummary[]> {
    return (await api.get<DocumentSummary[]>('/documents')).data
  },
  async get(id: string): Promise<DocumentDetail> {
    return (await api.get<DocumentDetail>(`/documents/${id}`)).data
  },
  async upload(kind: DocumentKind, file: File): Promise<DocumentSummary> {
    const form = new FormData()
    form.append('kind', kind)
    form.append('file', file)
    return (await api.post<DocumentSummary>('/documents', form)).data
  },
  async remove(id: string): Promise<{ ok: boolean }> {
    return (await api.delete(`/documents/${id}`)).data
  },
  async reprocess(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/documents/${id}/reprocess`)).data
  },
}

export const matchesApi = {
  async list(status?: string): Promise<MatchDetail[]> {
    return (await api.get<MatchDetail[]>('/matches', {
      params: status ? { status } : {},
    })).data
  },
  async suggest(): Promise<{ suggested: number }> {
    return (await api.post('/matches/suggest')).data
  },
  async accept(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/matches/${id}/accept`)).data
  },
  async reject(id: string): Promise<{ ok: boolean }> {
    return (await api.post(`/matches/${id}/reject`)).data
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

export const dashboardApi = {
  async get(month: string): Promise<DashboardData> {
    return (await api.get<DashboardData>('/dashboard', { params: { month } })).data
  },
  async trend(months = 12): Promise<TrendPoint[]> {
    return (await api.get<TrendPoint[]>('/dashboard/trend', { params: { months } })).data
  },
}

export const sourcesApi = {
  async list(): Promise<Source[]> {
    return (await api.get<Source[]>('/sources')).data
  },
  async update(id: string, input: { name?: string; enabled?: boolean }): Promise<Source> {
    return (await api.patch<Source>(`/sources/${id}`, input)).data
  },
}

export const coverageApi = {
  async get(): Promise<CoverageData> {
    return (await api.get<CoverageData>('/coverage')).data
  },
}

export const tagsApi = {
  async list(): Promise<string[]> {
    return (await api.get<string[]>('/tags')).data
  },
}

export interface MemoryEntry {
  id: string
  merchant: string
  category_id: string | null
  category_name: string | null
  confidence: number
  confirm_count: number
  last_confirmed_at: string | null
}

export interface MemoryInput {
  merchant: string
  category_id: string | null
}

export interface MemoryUpdateInput {
  category_id: string | null
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
  async applyAll(merchant: string): Promise<{ updated: number }> {
    return (await api.post('/memory/apply-all', { merchant })).data
  },
  async applyAllGlobal(): Promise<{ updated: number }> {
    return (await api.post('/memory/apply-all-global')).data
  },
}

export interface RecurringInput {
  merchant?: string | null
  description: string
  amount_cents: number
  category_id?: string | null
  frequency: string
  interval?: number
  day_of_month?: number | null
  next_due_on?: string | null
  is_active?: boolean
}

export const recurringApi = {
  async list(): Promise<RecurringRule[]> {
    return (await api.get<RecurringRule[]>('/recurring')).data
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
  async upcoming(): Promise<UpcomingOccurrence[]> {
    return (await api.get<UpcomingOccurrence[]>('/recurring/upcoming')).data
  },
  async suggestions(): Promise<RecurringSuggestion[]> {
    return (await api.get<RecurringSuggestion[]>('/recurring/suggestions')).data
  },
}
