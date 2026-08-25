import type { Category } from '../lib/types'

export interface ItemFiltersValue {
  search: string
  dateFrom: string
  dateTo: string
  categoryId: string
  tagFilter: string
  bankFilter: string
  kindFilter: string
  installments: 'all' | 'first_only' | 'only'
  sortBy: string
}

export const emptyFilters = (): ItemFiltersValue => ({
  search: '',
  dateFrom: '',
  dateTo: '',
  categoryId: '',
  tagFilter: '',
  bankFilter: '',
  kindFilter: '',
  installments: 'all',
  sortBy: 'date',
})

const BANK_LABELS: Record<string, string> = {
  nubank: 'Nubank',
  c6: 'C6',
  caixa: 'Caixa',
}

const KIND_OPTIONS: [string, string][] = [
  ['expense', 'Despesas'],
  ['income', 'Receitas'],
  ['internal', 'Internas'],
]

interface Props {
  value: ItemFiltersValue
  onChange: (next: ItemFiltersValue) => void
  categories: Category[]
  allTags: string[]
  banks: string[]
  /** Show the sort select (list-only; charts don't need it). */
  showSort?: boolean
  searchPlaceholder?: string
}

const set = (v: ItemFiltersValue, patch: Partial<ItemFiltersValue>): ItemFiltersValue => ({
  ...v,
  ...patch,
})

export default function ItemFilters({
  value,
  onChange,
  categories,
  allTags,
  banks,
  showSort = false,
  searchPlaceholder = 'Buscar…',
}: Props) {
  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <input
          value={value.search}
          onChange={(e) => onChange(set(value, { search: e.target.value }))}
          placeholder={searchPlaceholder}
          className="field w-72"
        />
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-1">
          <span className="text-xs text-zinc-500">De</span>
          <input
            type="date"
            value={value.dateFrom}
            onChange={(e) => onChange(set(value, { dateFrom: e.target.value }))}
            title="Data inicial (inclusive)"
            className="field w-full min-[480px]:w-40"
          />
        </div>
        <div className="flex items-center gap-1">
          <span className="text-xs text-zinc-500">Até</span>
          <input
            type="date"
            value={value.dateTo}
            onChange={(e) => onChange(set(value, { dateTo: e.target.value }))}
            title="Data final (inclusive)"
            className="field w-full min-[480px]:w-40"
          />
        </div>
        <select
          value={value.categoryId}
          onChange={(e) => onChange(set(value, { categoryId: e.target.value }))}
          className="field w-full min-[480px]:w-44"
        >
          <option value="">Todas categorias</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <select
          value={value.tagFilter}
          onChange={(e) => onChange(set(value, { tagFilter: e.target.value }))}
          className="field w-full min-[480px]:w-36"
        >
          <option value="">Todas tags</option>
          {allTags.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        <select
          value={value.bankFilter}
          onChange={(e) => onChange(set(value, { bankFilter: e.target.value }))}
          className="field w-full min-[480px]:w-36"
        >
          <option value="">Todos bancos</option>
          {banks.map((b) => (
            <option key={b} value={b}>
              {BANK_LABELS[b] ?? b}
            </option>
          ))}
        </select>
        <select
          value={value.kindFilter}
          onChange={(e) => onChange(set(value, { kindFilter: e.target.value }))}
          className="field w-full min-[480px]:w-36"
        >
          <option value="">Tipo: todos</option>
          {KIND_OPTIONS.map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        {showSort && (
          <select
            value={value.sortBy}
            onChange={(e) => onChange(set(value, { sortBy: e.target.value }))}
            className="field w-full min-[480px]:w-36"
          >
            <option value="date">Ordenar: data</option>
            <option value="value">Ordenar: valor</option>
          </select>
        )}
        <select
          value={value.installments}
          onChange={(e) =>
            onChange(
              set(value, { installments: e.target.value as 'all' | 'first_only' | 'only' }),
            )
          }
          title="Filtrar itens por parcelamento"
          className="field w-full min-[480px]:w-56"
        >
          <option value="all">Parcelas: todas</option>
          <option value="first_only">Parcelas: só a 1ª de cada</option>
          <option value="only">Parcelas: apenas compras parceladas</option>
        </select>
      </div>
    </>
  )
}
