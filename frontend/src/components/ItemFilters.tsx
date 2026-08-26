import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { useSearchParams } from 'react-router-dom'
import type { Category } from '../lib/types'
import PeriodPicker from './PeriodPicker'

export interface ItemFiltersValue {
  search: string
  dateFrom: string
  dateTo: string
  /** Multiple categories (OR). */
  categoryIds: string[]
  /** Multiple tags (OR: item carries any). */
  tagFilter: string[]
  bankFilter: string
  kindFilter: string
  installments: 'all' | 'first_only' | 'only'
  sortBy: string
}

export const emptyFilters = (): ItemFiltersValue => ({
  search: '',
  dateFrom: '',
  dateTo: '',
  categoryIds: [],
  tagFilter: [],
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

const KIND_LABELS: Record<string, string> = {
  expense: 'Despesas',
  income: 'Receitas',
  internal: 'Internas',
}

const KIND_OPTIONS: [string, string][] = [
  ['', 'Todas'],
  ['expense', 'Despesas'],
  ['income', 'Receitas'],
  ['internal', 'Internas'],
]

const INSTALLMENT_LABELS: Record<string, string> = {
  first_only: '1ª de cada',
  only: 'Só parceladas',
}

const INSTALLMENT_OPTIONS: [string, string][] = [
  ['all', 'Todas'],
  ['first_only', '1ª de cada'],
  ['only', 'Só parceladas'],
]

const VALID_KINDS = new Set(['expense', 'income', 'internal'])
const VALID_INSTALLMENTS = new Set(['first_only', 'only'])

/** Sentinel value (kept inside the comma-separated filter params) for
 * "no category" / "no tags" — must match the backend (`routes::items::NO_FILTER`). */
const NO_FILTER = '__none'

/**
 * Filters live in the URL (?q=…&category_ids=a&category_ids=b&tags=x&tags=y…),
 * so filtered views are shareable/bookmarkable. Multi-values use repeated keys.
 * `setFilters(next, { replace: true })` avoids polluting history (used for the
 * search box — one entry per keystroke would be awful); discrete changes push.
 */
export function useFiltersUrl() {
  const [searchParams, setSearchParams] = useSearchParams()

  const filters = useMemo<ItemFiltersValue>(() => {
    const kind = searchParams.get('kind')
    const installments = searchParams.get('installments')
    return {
      search: searchParams.get('q') ?? '',
      dateFrom: searchParams.get('date_from') ?? '',
      dateTo: searchParams.get('date_to') ?? '',
      categoryIds: (searchParams.get('category_ids') ?? '')
        .split(',')
        .filter(Boolean),
      tagFilter: (searchParams.get('tags') ?? '').split(',').filter(Boolean),
      bankFilter: searchParams.get('bank') ?? '',
      kindFilter: (kind && VALID_KINDS.has(kind) ? kind : '') as ItemFiltersValue['kindFilter'],
      installments: (installments && VALID_INSTALLMENTS.has(installments)
        ? installments
        : 'all') as ItemFiltersValue['installments'],
      sortBy: searchParams.get('sort') === 'value' ? 'value' : 'date',
    }
  }, [searchParams])

  const setFilters = useCallback(
    (next: ItemFiltersValue, opts?: { replace?: boolean }) => {
      const params = new URLSearchParams()
      if (next.search) params.set('q', next.search)
      if (next.dateFrom) params.set('date_from', next.dateFrom)
      if (next.dateTo) params.set('date_to', next.dateTo)
      if (next.categoryIds.length) params.set('category_ids', next.categoryIds.join(','))
      if (next.tagFilter.length) params.set('tags', next.tagFilter.join(','))
      if (next.bankFilter) params.set('bank', next.bankFilter)
      if (next.kindFilter) params.set('kind', next.kindFilter)
      if (next.installments !== 'all') params.set('installments', next.installments)
      if (next.sortBy !== 'date') params.set('sort', next.sortBy)
      setSearchParams(params, { replace: opts?.replace ?? false })
    },
    [setSearchParams],
  )

  return { filters, setFilters }
}

interface Props {
  value: ItemFiltersValue
  onChange: (next: ItemFiltersValue, opts?: { replace?: boolean }) => void
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

function FieldLabel({ children }: { children: ReactNode }) {
  return (
    <span className="mb-1 block text-[10px] font-medium uppercase tracking-wide text-zinc-500">
      {children}
    </span>
  )
}

function Segmented({
  options,
  value,
  onSelect,
}: {
  options: [string, string][]
  value: string
  onSelect: (v: string) => void
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {options.map(([v, label]) => (
        <button
          key={v}
          type="button"
          onClick={() => onSelect(v)}
          className={`rounded-full px-3 py-1.5 text-xs font-medium transition-colors ${
            value === v
              ? 'bg-zinc-100 text-zinc-900'
              : 'text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200'
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  )
}

/** Dropdown with checkboxes — multi-select for categories and tags. */
function MultiSelect({
  label,
  emptyLabel,
  options,
  selected,
  onToggle,
}: {
  label: string
  emptyLabel: string
  options: { value: string; label: string }[]
  selected: string[]
  onToggle: (v: string) => void
}) {
  const [open, setOpen] = useState(false)
  const count = selected.length
  return (
    <div className="relative">
      <FieldLabel>{label}</FieldLabel>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="field flex items-center justify-between gap-2 text-left"
      >
        <span className="truncate">
          {count === 0 ? emptyLabel : `${count} selecionada${count > 1 ? 's' : ''}`}
        </span>
        <span className={`shrink-0 text-zinc-500 transition-transform ${open ? 'rotate-180' : ''}`}>
          ▾
        </span>
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div className="absolute left-0 right-0 z-20 mt-1 max-h-64 overflow-y-auto rounded-md border border-zinc-700 bg-zinc-900 py-1 shadow-xl">
            {options.length === 0 && (
              <p className="px-3 py-1.5 text-sm text-zinc-500">Nenhuma opção</p>
            )}
            {options.map((o) => (
              <label
                key={o.value}
                className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm hover:bg-zinc-800"
              >
                <input
                  type="checkbox"
                  checked={selected.includes(o.value)}
                  onChange={() => onToggle(o.value)}
                  className="checkbox"
                />
                <span className="truncate">{o.label}</span>
              </label>
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function fmtDay(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}`
}

export default function ItemFilters({
  value,
  onChange,
  categories,
  allTags,
  banks,
  showSort = false,
  searchPlaceholder = 'Buscar…',
}: Props) {
  // Active filters, as removable chips ("Limpar todos" resets everything).
  const active: { key: string; label: string; clear: () => void }[] = []

  if (value.search) {
    active.push({
      key: 'q',
      label: `Busca: ${value.search}`,
      clear: () => onChange(set(value, { search: '' }), { replace: true }),
    })
  }
  if (value.dateFrom || value.dateTo) {
    const label = value.dateFrom && value.dateTo
      ? `${fmtDay(value.dateFrom)} a ${fmtDay(value.dateTo)}`
      : value.dateFrom
        ? `Desde ${fmtDay(value.dateFrom)}`
        : `Até ${fmtDay(value.dateTo)}`
    active.push({
      key: 'periodo',
      label,
      clear: () => onChange(set(value, { dateFrom: '', dateTo: '' }), { replace: true }),
    })
  }
  const catName = new Map(categories.map((c) => [c.id, c.name]))
  value.categoryIds.forEach((id) => {
    const name = id === NO_FILTER ? 'Sem categoria' : catName.get(id)
    if (name) {
      active.push({
        key: `cat-${id}`,
        label: `Categoria: ${name}`,
        clear: () =>
          onChange(set(value, { categoryIds: value.categoryIds.filter((x) => x !== id) })),
      })
    }
  })
  value.tagFilter.forEach((t) => {
    const label = t === NO_FILTER ? 'Sem tags' : t
    active.push({
      key: `tag-${t}`,
      label: `Tag: ${label}`,
      clear: () => onChange(set(value, { tagFilter: value.tagFilter.filter((x) => x !== t) })),
    })
  })
  if (value.bankFilter) {
    active.push({
      key: 'bank',
      label: `Banco: ${BANK_LABELS[value.bankFilter] ?? value.bankFilter}`,
      clear: () => onChange(set(value, { bankFilter: '' })),
    })
  }
  if (value.kindFilter) {
    active.push({
      key: 'kind',
      label: `Tipo: ${KIND_LABELS[value.kindFilter] ?? value.kindFilter}`,
      clear: () => onChange(set(value, { kindFilter: '' })),
    })
  }
  if (value.installments !== 'all') {
    active.push({
      key: 'installments',
      label: `Parcelas: ${INSTALLMENT_LABELS[value.installments] ?? value.installments}`,
      clear: () => onChange(set(value, { installments: 'all' })),
    })
  }

  return (
    <>
      <div className="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <div className="sm:col-span-2 lg:col-span-4">
          <FieldLabel>Buscar</FieldLabel>
          <input
            value={value.search}
            onChange={(e) => onChange(set(value, { search: e.target.value }), { replace: true })}
            placeholder={searchPlaceholder}
            className="field"
          />
        </div>

        <PeriodPicker
          dateFrom={value.dateFrom}
          dateTo={value.dateTo}
          onChange={(from, to, opts) => onChange(set(value, { dateFrom: from, dateTo: to }), opts)}
        />

        <MultiSelect
          label="Categorias"
          emptyLabel="Todas"
          options={[
            { value: NO_FILTER, label: 'Sem categoria' },
            ...categories.map((c) => ({ value: c.id, label: c.name })),
          ]}
          selected={value.categoryIds}
          onToggle={(id) =>
            onChange(
              set(value, {
                categoryIds: value.categoryIds.includes(id)
                  ? value.categoryIds.filter((x) => x !== id)
                  : [...value.categoryIds, id],
              }),
            )
          }
        />

        <MultiSelect
          label="Tags"
          emptyLabel="Todas"
          options={[
            { value: NO_FILTER, label: 'Sem tags' },
            ...allTags.map((t) => ({ value: t, label: t })),
          ]}
          selected={value.tagFilter}
          onToggle={(t) =>
            onChange(
              set(value, {
                tagFilter: value.tagFilter.includes(t)
                  ? value.tagFilter.filter((x) => x !== t)
                  : [...value.tagFilter, t],
              }),
            )
          }
        />

        <div>
          <FieldLabel>Banco</FieldLabel>
          <select
            value={value.bankFilter}
            onChange={(e) => onChange(set(value, { bankFilter: e.target.value }))}
            className="field"
          >
            <option value="">Todos</option>
            {banks.map((b) => (
              <option key={b} value={b}>
                {BANK_LABELS[b] ?? b}
              </option>
            ))}
          </select>
        </div>

        {showSort && (
          <div>
            <FieldLabel>Ordenar</FieldLabel>
            <select
              value={value.sortBy}
              onChange={(e) => onChange(set(value, { sortBy: e.target.value }))}
              className="field"
            >
              <option value="date">Data</option>
              <option value="value">Valor</option>
            </select>
          </div>
        )}

        <div className="sm:col-span-2">
          <FieldLabel>Tipo</FieldLabel>
          <Segmented
            options={KIND_OPTIONS}
            value={value.kindFilter}
            onSelect={(v) => onChange(set(value, { kindFilter: v }))}
          />
        </div>

        <div className="sm:col-span-2">
          <FieldLabel>Parcelas</FieldLabel>
          <Segmented
            options={INSTALLMENT_OPTIONS}
            value={value.installments}
            onSelect={(v) =>
              onChange(set(value, { installments: v as 'all' | 'first_only' | 'only' }))
            }
          />
        </div>
      </div>

      {active.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-1.5">
          {active.map((c) => (
            <span
              key={c.key}
              className="flex items-center gap-1 rounded-full border border-zinc-700 bg-zinc-900 px-2.5 py-1 text-xs text-zinc-300"
            >
              {c.label}
              <button
                type="button"
                onClick={c.clear}
                title="Remover filtro"
                className="text-zinc-500 hover:text-zinc-100"
              >
                ×
              </button>
            </span>
          ))}
          <button
            type="button"
            onClick={() => onChange(emptyFilters(), { replace: true })}
            className="rounded-full px-2 py-1 text-xs text-zinc-500 hover:text-red-400"
          >
            Limpar todos
          </button>
        </div>
      )}
    </>
  )
}
