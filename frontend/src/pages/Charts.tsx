import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
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
  recurringApi,
  sourcesApi,
  tagsApi,
} from '../api/client'
import { fmtCents, lastCompleteMonthRange } from '../lib/format'
import ItemFilters, { emptyFilters, type ItemFiltersValue } from '../components/ItemFilters'

const PIE_COLORS = [
  '#34d399', '#60a5fa', '#f472b6', '#f87171', '#fbbf24', '#a78bfa',
  '#22d3ee', '#fb923c', '#a3e635', '#e879f9',
]

function fmtShort(cents: number): string {
  const v = Math.abs(cents) / 100
  if (v >= 1000) {
    return `${(v / 1000).toLocaleString('pt-BR', { maximumFractionDigits: 1 })}k`
  }
  return v.toLocaleString('pt-BR', { maximumFractionDigits: 0 })
}

function dashboardParams(f: ItemFiltersValue) {
  return {
    date_from: f.dateFrom || undefined,
    date_to: f.dateTo || undefined,
    search: f.search || undefined,
    category_id: f.categoryId || undefined,
    tag: f.tagFilter || undefined,
    bank: f.bankFilter || undefined,
    kind: f.kindFilter || undefined,
    installments: f.installments === 'all' ? undefined : f.installments,
  }
}

export default function Charts() {
  const [graphsOpen, setGraphsOpen] = useState(true)
  const [filters, setFilters] = useState<ItemFiltersValue>(() => {
    const { from, to } = lastCompleteMonthRange()
    return { ...emptyFilters(), dateFrom: from, dateTo: to }
  })

  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })
  const { data: allTags = [] } = useQuery({ queryKey: ['tags'], queryFn: tagsApi.list })
  const { data: sources = [] } = useQuery({ queryKey: ['sources'], queryFn: sourcesApi.list })
  const { data: coverage } = useQuery({ queryKey: ['coverage'], queryFn: coverageApi.get })

  const { data: dash } = useQuery({
    queryKey: ['dashboard', dashboardParams(filters)],
    queryFn: () => dashboardApi.get(dashboardParams(filters)),
  })
  const { data: trend = [] } = useQuery({
    queryKey: ['dashboard-trend', dashboardParams(filters)],
    queryFn: () => dashboardApi.trend(12, dashboardParams(filters)),
  })
  const { data: upcoming = [] } = useQuery({
    queryKey: ['recurring-upcoming'],
    queryFn: recurringApi.upcoming,
  })

  const banks = [...new Set(sources.map((s) => s.bank))].sort()

  // The missing-sources alert is month-specific: only show it when the whole
  // range falls inside a single calendar month.
  const rangeMonth =
    filters.dateFrom && filters.dateTo && filters.dateFrom.slice(0, 7) === filters.dateTo.slice(0, 7)
      ? filters.dateTo.slice(0, 7)
      : null
  const missingSources = rangeMonth
    ? (coverage?.sources ?? []).filter((s) => s.enabled && !s.present.includes(rangeMonth))
    : []

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

  const rangeLabel = useMemo(() => {
    const { from } = lastCompleteMonthRange()
    if (!filters.dateFrom && !filters.dateTo) {
      return `último mês completo (${from.slice(0, 7)})`
    }
    if (filters.dateFrom && filters.dateTo && filters.dateFrom.slice(0, 7) === filters.dateTo.slice(0, 7)) {
      return filters.dateTo.slice(0, 7)
    }
    return `${filters.dateFrom || '…'} a ${filters.dateTo || '…'}`
  }, [filters.dateFrom, filters.dateTo])

  return (
    <div className="pb-20">
      <div className="mb-4 flex items-baseline gap-3">
        <h1 className="text-xl font-bold">Gráficos</h1>
        <p className="text-xs text-zinc-500">
          Período: {rangeLabel}
        </p>
      </div>

      <ItemFilters
        value={filters}
        onChange={setFilters}
        categories={categories}
        allTags={allTags}
        banks={banks}
        searchPlaceholder="Buscar nos gráficos…"
      />

      {missingSources.length > 0 && (
        <div className="mb-4 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-300">
          Fontes faltando em {rangeMonth}: {missingSources.map((s) => s.name).join(', ')}
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
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Histórico (12 meses)</h2>
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

            <p className="mt-3 text-center text-xs text-zinc-600">
              <Link to="/lista" className="hover:text-zinc-300">
                Ver todos os itens na Lista →
              </Link>
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
