import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  CartesianGrid,
  Cell,
  Legend,
  Line,
  LineChart,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import {
  categoriesApi,
  coverageApi,
  dashboardApi,
  documentsApi,
  itemsApi,
  recurringApi,
  sourcesApi,
  tagsApi,
  type BulkItemUpdateInput,
} from '../api/client'
import type { Item } from '../lib/types'
import { currentMonth, fmtCents } from '../lib/format'
import ItemForm from '../components/ItemForm'
import BulkEditModal from '../components/BulkEditModal'
import BankLogo from '../components/BankLogo'

interface FormState {
  open: boolean
  parent?: Item | null
  editing?: Item | null
}

const KIND_LABELS: Record<string, string> = {
  income: 'Receita',
  refund: 'Estorno',
  card_payment: 'Fatura',
  investment: 'Investimento',
  internal: 'Interna',
}

const PIE_COLORS = [
  '#34d399', '#60a5fa', '#f472b6', '#f87171', '#fbbf24', '#a78bfa',
  '#22d3ee', '#fb923c', '#a3e635', '#e879f9',
]

function shiftMonth(ym: string, delta: number): string {
  const [y, m] = ym.split('-').map(Number)
  const d = new Date(y, m - 1 + delta, 1)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
}

function monthLabel(ym: string): string {
  const [y, m] = ym.split('-').map(Number)
  return new Date(y, m - 1, 1).toLocaleDateString('pt-BR', {
    month: 'long',
    year: 'numeric',
  })
}

function shortDate(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}`
}

function fmtShort(cents: number): string {
  const v = Math.abs(cents) / 100
  if (v >= 1000) {
    return `${(v / 1000).toLocaleString('pt-BR', { maximumFractionDigits: 1 })}k`
  }
  return v.toLocaleString('pt-BR', { maximumFractionDigits: 0 })
}

function itemTitle(it: Item): string {
  if (it.merchant) return it.merchant
  const parts = it.description.split(' - ')
  const idx = (parts[0] ?? '').toLowerCase().includes('estorno') ? 2 : 1
  const candidate = parts[idx]?.trim()
  if (candidate && candidate.replace(/\D/g, '').length !== 14) {
    return candidate
  }
  return it.description
}

function amountColor(cents: number): string {
  if (cents < 0) return 'text-red-400'
  if (cents > 0) return 'text-emerald-400'
  return 'text-zinc-400'
}

function signOf(cents: number): string {
  return cents > 0 ? '+' : cents < 0 ? '−' : ''
}

export default function MonthView() {
  const { ym } = useParams()
  const navigate = useNavigate()
  const month = ym ?? currentMonth()

  const [form, setForm] = useState<FormState>({ open: false })
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [bulkOpen, setBulkOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [categoryId, setCategoryId] = useState('')
  const [tagFilter, setTagFilter] = useState('')
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [bankFilter, setBankFilter] = useState('')
  const [kindFilter, setKindFilter] = useState('')
  const [sortBy, setSortBy] = useState('date')
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [detailsFor, setDetailsFor] = useState<string | null>(null)
  const [graphsOpen, setGraphsOpen] = useState(true)
  const [listOpen, setListOpen] = useState(true)
  const qc = useQueryClient()

  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })
  const { data: sources = [] } = useQuery({ queryKey: ['sources'], queryFn: sourcesApi.list })
  const { data: allTags = [] } = useQuery({ queryKey: ['tags'], queryFn: tagsApi.list })
  const { data: docs = [] } = useQuery({ queryKey: ['documents'], queryFn: documentsApi.list })
  const { data: coverage } = useQuery({ queryKey: ['coverage'], queryFn: coverageApi.get })

  const { data: items = [], isLoading, isPlaceholderData } = useQuery({
    queryKey: ['items', month, search, categoryId, tagFilter, bankFilter, kindFilter, sortBy],
    queryFn: () =>
      itemsApi.list({
        month,
        search: search || undefined,
        category_id: categoryId || undefined,
        tag: tagFilter || undefined,
        bank: bankFilter || undefined,
        kind: kindFilter || undefined,
        sort: sortBy || undefined,
      }),
    // Keep the previous list rendered while a filter change refetches, so the
    // page doesn't collapse to a loading line and the scroll position holds.
    placeholderData: keepPreviousData,
  })

  const { data: dash } = useQuery({
    queryKey: ['dashboard', month],
    queryFn: () => dashboardApi.get(month),
  })
  const { data: trend = [] } = useQuery({
    queryKey: ['dashboard-trend'],
    queryFn: () => dashboardApi.trend(12),
  })
  const { data: upcoming = [] } = useQuery({
    queryKey: ['recurring-upcoming'],
    queryFn: recurringApi.upcoming,
  })

  const del = useMutation({
    mutationFn: (id: string) => itemsApi.remove(id),
    onSuccess: (_data, id) => {
      qc.invalidateQueries({ queryKey: ['items', month] })
      // Drop a deleted item from the selection so the count stays accurate.
      setSelected((prev) => {
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    },
  })

  const bulk = useMutation({
    mutationFn: (input: BulkItemUpdateInput) => itemsApi.bulkUpdate(input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['items'] })
      qc.invalidateQueries({ queryKey: ['dashboard'] })
      qc.invalidateQueries({ queryKey: ['tags'] })
      qc.invalidateQueries({ queryKey: ['memory'] })
      setBulkOpen(false)
      setSelected(new Set())
    },
  })

  // Selection is scoped to what's currently visible: changing the month or any
  // filter resets it (avoids accidentally bulk-editing hidden items).
  useEffect(() => {
    setSelected(new Set())
  }, [month, search, categoryId, tagFilter, bankFilter, kindFilter])

  const catById = new Map(categories.map((c) => [c.id, c]))
  const bankBySource = new Map(sources.map((s) => [s.id, s.bank]))
  const banks = [...new Set(sources.map((s) => s.bank))].sort()
  const bankByDoc = new Map(
    docs.map((d) => [d.id, d.source_id ? bankBySource.get(d.source_id) : undefined]),
  )

  const byParent = new Map<string | null, Item[]>()
  for (const it of items) {
    const k = it.parent_id
    if (!byParent.has(k)) byParent.set(k, [])
    byParent.get(k)!.push(it)
  }
  const childSum = new Map<string, number>()
  for (const it of items) {
    if (it.parent_id) {
      childSum.set(it.parent_id, (childSum.get(it.parent_id) ?? 0) + Math.abs(it.amount_cents))
    }
  }
  const roots = items.filter((i) => i.parent_id === null)
  const rootIds = roots.map((r) => r.id)
  const allSelected = roots.length > 0 && roots.every((r) => selected.has(r.id))

  const missingSources = (coverage?.sources ?? []).filter((s) => s.enabled && !s.present.includes(month))

  const pieData = (dash?.by_category ?? []).map((c) => ({
    name: c.name,
    value: c.total_cents,
    color: c.color,
  }))
  const lineData = trend.map((t) => ({
    name: `${t.month.slice(5, 7)}/${t.month.slice(0, 4)}`,
    Despesas: t.spend_cents,
    Receitas: t.income_cents,
  }))

  const renderSubItem = (c: Item) => (
    <div
      key={c.id}
      className="flex items-center gap-2 border-t border-zinc-800/60 py-1 first:border-t-0"
    >
      <span className="min-w-0 flex-1 truncate text-xs text-zinc-300">{c.description}</span>
      {c.tags.length > 0 && (
        <span className="flex shrink-0 items-center gap-1">
          {c.tags.slice(0, 3).map((t) => (
            <span
              key={t}
              className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400"
            >
              {t}
            </span>
          ))}
          {c.tags.length > 3 && (
            <span className="text-[10px] text-zinc-600">+{c.tags.length - 3}</span>
          )}
        </span>
      )}
      <span className={`shrink-0 text-xs tabular-nums ${amountColor(c.amount_cents)}`}>
        {signOf(c.amount_cents)}
        {fmtCents(c.amount_cents)}
      </span>
    </div>
  )

  const renderRoot = (it: Item) => {
    const children = byParent.get(it.id) ?? []
    const hasChildren = children.length > 0
    const receiptDocId = children[0]?.document_id
    const cat = it.category_id ? catById.get(it.category_id) : undefined
    const bank = it.document_id ? bankByDoc.get(it.document_id) : undefined
    const kindLabel = KIND_LABELS[it.kind]
    const open = menuFor === it.id
    const detailsOpen = detailsFor === it.id
    const allocated = childSum.get(it.id) ?? 0
    const remainder = Math.abs(it.amount_cents) - allocated

    return (
      <div key={it.id}>
        <div className="group relative flex items-center gap-2 py-1.5">
          <input
            type="checkbox"
            checked={selected.has(it.id)}
            onChange={(e) => {
              const next = new Set(selected)
              if (e.target.checked) {
                next.add(it.id)
              } else {
                next.delete(it.id)
              }
              setSelected(next)
            }}
            onClick={(e) => e.stopPropagation()}
            title="Selecionar para edição em massa"
            className="checkbox shrink-0"
          />
          <button
            onClick={() => setDetailsFor(detailsOpen ? null : it.id)}
            className="shrink-0 text-xs text-zinc-500 hover:text-zinc-200"
          >
            {detailsOpen ? '▾' : '▸'}
          </button>
          <BankLogo bank={bank} />
          <span className="w-10 shrink-0 text-xs tabular-nums text-zinc-500">
            {shortDate(it.occurred_on)}
          </span>
          <span className="min-w-0 flex-1 truncate text-sm" title={it.description}>
            {itemTitle(it)}
          </span>
          {hasChildren && (
            <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
              {children.length} itens
            </span>
          )}
          {kindLabel && (
            <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
              {kindLabel}
            </span>
          )}
          {cat && (
            <span className="flex shrink-0 items-center gap-1 text-xs text-zinc-400">
              {cat.color && <span className="h-2 w-2 rounded-full" style={{ background: cat.color }} />}
              {cat.name}
            </span>
          )}
          {it.tags.length > 0 && (
            <span className="flex shrink-0 items-center gap-1">
              {it.tags.slice(0, 2).map((t) => (
                <span
                  key={t}
                  className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400"
                >
                  {t}
                </span>
              ))}
              {it.tags.length > 2 && (
                <span className="text-[10px] text-zinc-600">+{it.tags.length - 2}</span>
              )}
            </span>
          )}
          <span className={`shrink-0 text-sm tabular-nums ${amountColor(it.amount_cents)}`}>
            {signOf(it.amount_cents)}
            {fmtCents(it.amount_cents)}
          </span>
          {receiptDocId && (
            <a
              href={`/api/documents/${receiptDocId}/file`}
              target="_blank"
              rel="noreferrer"
              title="Ver recibo"
              className="shrink-0 text-zinc-500 hover:text-zinc-200"
            >
              🧾
            </a>
          )}
          <button
            onClick={() => setMenuFor(open ? null : it.id)}
            className="shrink-0 px-1 text-zinc-500 hover:text-zinc-200"
          >
            ⋯
          </button>

          {open && (
            <>
              <div className="fixed inset-0 z-10" onClick={() => setMenuFor(null)} />
              <div className="absolute right-0 top-full z-20 mt-1 w-44 rounded border border-zinc-700 bg-zinc-900 py-1 shadow-xl">
                <button
                  onClick={() => {
                    setMenuFor(null)
                    setForm({ open: true, editing: it })
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                >
                  Editar
                </button>
                <button
                  onClick={() => {
                    setMenuFor(null)
                    setForm({ open: true, parent: it })
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                >
                  Adicionar sub-item
                </button>
                <button
                  onClick={() => {
                    setMenuFor(null)
                    del.mutate(it.id)
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm text-red-400 hover:bg-zinc-800"
                >
                  Apagar
                </button>
              </div>
            </>
          )}
        </div>

        {detailsOpen && (
          <div className="mb-1 rounded border border-zinc-800 bg-zinc-900/60 px-3 py-2 text-xs text-zinc-400">
            {hasChildren && (
              <div className="mb-2">
                {children.map(renderSubItem)}
                {remainder !== 0 && (
                  <div className="mt-1 text-right text-[11px] text-amber-400/80">
                    não alocado: {fmtCents(remainder)}
                  </div>
                )}
              </div>
            )}
            <p className="mb-1 whitespace-pre-wrap text-zinc-300">{it.description}</p>
            {it.merchant && <p>Comerciante: {it.merchant}</p>}
            {it.tags.length > 0 && (
              <p className="mb-1 flex flex-wrap items-center gap-1">
                <span>Tags:</span>
                {it.tags.map((t) => (
                  <span
                    key={t}
                    className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400"
                  >
                    {t}
                  </span>
                ))}
              </p>
            )}
            {it.installment_count != null && (
              <p>
                Parcela: {it.installment}/{it.installment_count}
              </p>
            )}
            <p>
              Fonte: {it.source} · Tipo: {it.kind}
            </p>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="pb-20">
      <div className="mb-4 flex h-9 items-center overflow-hidden rounded-md border border-zinc-700 bg-zinc-950">
        <button
          onClick={() => navigate(`/months/${shiftMonth(month, -1)}`)}
          className="h-full px-3 text-base text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
        >
          ‹
        </button>
        <span className="flex-1 text-center text-sm font-medium capitalize">
          {monthLabel(month)}
        </span>
        <button
          onClick={() => navigate(`/months/${shiftMonth(month, 1)}`)}
          className="h-full px-3 text-base text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
        >
          ›
        </button>
      </div>

      {missingSources.length > 0 && (
        <div className="mb-4 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-300">
          Fontes faltando em {month}: {missingSources.map((s) => s.name).join(', ')}
        </div>
      )}

      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setGraphsOpen(!graphsOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Gráficos
          <span className="ml-auto text-zinc-500">{graphsOpen ? '▾' : '▸'}</span>
        </button>
        {graphsOpen && (
          <div className="border-t border-zinc-800 p-4">
            <div className="mb-4 grid grid-cols-4 gap-3">
              <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
                <p className="text-xs text-zinc-500">Despesas</p>
                <p className="text-lg font-semibold tabular-nums">{fmtCents(dash?.total_spend_cents ?? 0)}</p>
              </div>
              <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
                <p className="text-xs text-zinc-500">Receitas</p>
                <p className="text-lg font-semibold tabular-nums text-emerald-400">
                  {fmtCents(dash?.total_income_cents ?? 0)}
                </p>
              </div>
              <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
                <p className="text-xs text-zinc-500">Pendentes</p>
                <p className="text-lg font-semibold tabular-nums text-amber-400">{dash?.pending_count ?? 0}</p>
              </div>
              <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
                <p className="text-xs text-zinc-500">Recorrentes</p>
                <p className="text-lg font-semibold tabular-nums text-cyan-400">{upcoming.length}</p>
              </div>
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Gastos por categoria</h2>
                {pieData.length === 0 ? (
                  <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
                ) : (
                  <ResponsiveContainer width="100%" height={240}>
                    <PieChart>
                      <Pie data={pieData} dataKey="value" nameKey="name" innerRadius={50} outerRadius={80} paddingAngle={2}>
                        {pieData.map((entry, i) => (
                          <Cell key={i} fill={entry.color || PIE_COLORS[i % PIE_COLORS.length]} />
                        ))}
                      </Pie>
                      <Tooltip formatter={(v) => fmtCents(Number(v))} />
                      <Legend />
                    </PieChart>
                  </ResponsiveContainer>
                )}
              </div>

              <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Últimos 12 meses</h2>
                <ResponsiveContainer width="100%" height={240}>
                  <LineChart data={lineData}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#27272a" />
                    <XAxis dataKey="name" tick={{ fontSize: 10 }} stroke="#52525b" />
                    <YAxis tick={{ fontSize: 10 }} stroke="#52525b" tickFormatter={(v) => fmtShort(Number(v))} />
                    <Tooltip formatter={(v) => fmtCents(Number(v))} />
                    <Legend />
                    <Line type="monotone" dataKey="Despesas" stroke="#f87171" dot={false} />
                    <Line type="monotone" dataKey="Receitas" stroke="#34d399" dot={false} />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </div>

            {(dash?.top_merchants ?? []).length > 0 && (
              <div className="mt-3 rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Maiores comerciantes</h2>
                <ul className="space-y-1">
                  {dash!.top_merchants.map((m) => (
                    <li key={m.merchant} className="flex items-center gap-3 text-sm">
                      <span className="truncate">{m.merchant}</span>
                      <span className="ml-auto tabular-nums">{fmtCents(m.total_cents)}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        )}
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Buscar…"
          className="field w-64"
        />
        <button
          onClick={() => setFiltersOpen(!filtersOpen)}
          className="flex items-center gap-1 rounded border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm leading-normal text-zinc-300 hover:bg-zinc-800"
        >
          Filtros
          <span className="text-zinc-500">{filtersOpen ? '▾' : '▸'}</span>
        </button>
      </div>

      {filtersOpen && (
        <div className="mb-4 flex flex-wrap items-center gap-3">
          <select
            value={categoryId}
            onChange={(e) => setCategoryId(e.target.value)}
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
            value={tagFilter}
            onChange={(e) => setTagFilter(e.target.value)}
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
            value={bankFilter}
            onChange={(e) => setBankFilter(e.target.value)}
            className="field w-full min-[480px]:w-36"
          >
            <option value="">Todos bancos</option>
            {banks.map((b) => (
              <option key={b} value={b}>
                {b === 'nubank' ? 'Nubank' : b === 'c6' ? 'C6' : b === 'caixa' ? 'Caixa' : b}
              </option>
            ))}
          </select>
          <select
            value={kindFilter}
            onChange={(e) => setKindFilter(e.target.value)}
            className="field w-full min-[480px]:w-36"
          >
            <option value="">Tipo: todos</option>
            <option value="expense">Despesas</option>
            <option value="income">Receitas</option>
            <option value="internal">Internas</option>
          </select>
          <select
            value={sortBy}
            onChange={(e) => setSortBy(e.target.value)}
            className="field w-full min-[480px]:w-36"
          >
            <option value="date">Ordenar: data</option>
            <option value="value">Ordenar: valor</option>
          </select>
        </div>
      )}

      {selected.size > 0 && (
        <div className="fixed bottom-4 left-1/2 z-10 flex -translate-x-1/2 items-center gap-3 rounded-full border border-zinc-700 bg-zinc-900/95 py-2 pl-5 pr-2 text-sm shadow-2xl shadow-black/50 backdrop-blur">
          <span className="font-medium text-zinc-100">
            {selected.size} selecionado{selected.size > 1 ? 's' : ''}
          </span>
          <button
            onClick={() => setBulkOpen(true)}
            className="rounded-full bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-zinc-200"
          >
            Editar seleção
          </button>
          <button
            onClick={() => setSelected(new Set())}
            className="rounded-full px-3 py-1.5 text-sm text-zinc-400 hover:text-zinc-100"
          >
            Limpar
          </button>
        </div>
      )}

      <div className="rounded border border-zinc-800 bg-zinc-900">
        <div className="flex items-center">
          <button
            onClick={() => setListOpen(!listOpen)}
            className="flex flex-1 items-center gap-2 px-4 py-3 text-sm font-medium"
          >
            Itens
            <span className="ml-auto text-zinc-500">{listOpen ? '▾' : '▸'}</span>
          </button>
          {roots.length > 0 && (
            <button
              onClick={() => setSelected(allSelected ? new Set() : new Set(rootIds))}
              className="px-4 py-3 text-xs text-zinc-400 hover:text-zinc-100"
            >
              {allSelected ? 'Limpar seleção' : 'Selecionar todos'}
            </button>
          )}
        </div>
        {listOpen && (
          <div
            className={`border-t border-zinc-800 px-2 py-2 transition-opacity ${isPlaceholderData ? 'opacity-60' : ''}`}
          >
            {isLoading ? (
              <p className="px-2 text-zinc-500">carregando…</p>
            ) : roots.length === 0 ? (
              <p className="px-2 text-zinc-500">Nenhum item neste mês.</p>
            ) : (
              <div className="divide-y divide-zinc-900">{roots.map(renderRoot)}</div>
            )}
          </div>
        )}
      </div>

      {bulkOpen && (
        <BulkEditModal
          ids={[...selected]}
          onClose={() => setBulkOpen(false)}
          onApply={(input) => bulk.mutate(input)}
        />
      )}
      {form.open && (
        <ItemForm
          month={month}
          parent={form.parent}
          editing={form.editing}
          onClose={() => {
            setForm({ open: false })
            qc.invalidateQueries({ queryKey: ['items'] })
            qc.invalidateQueries({ queryKey: ['dashboard'] })
          }}
        />
      )}
    </div>
  )
}
