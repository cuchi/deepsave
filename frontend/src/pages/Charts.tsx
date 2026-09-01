import { useEffect, useMemo, useState } from 'react'
import { Link, useLocation, useSearchParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  banksApi,
  categoriesApi,
  dashboardApi,
  recurringApi,
  tagsApi,
  type CategoryTotal,
  type DailyPoint,
  type TagTotal,
  type TrendPoint,
} from '../api/client'
import { currentMonth, currentMonthRange, fmtCents } from '../lib/format'
import ItemFilters, { useFiltersUrl, type ItemFiltersValue } from '../components/ItemFilters'
import EChart, { type EChartsCoreOption } from '../components/EChart'

const fmtUpdatedAt = (iso?: string) => {
  if (!iso) return '—'
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

const PIE_COLORS = [
  '#34d399', '#60a5fa', '#f472b6', '#f87171', '#fbbf24', '#a78bfa',
  '#22d3ee', '#fb923c', '#a3e635', '#e879f9',
]

const AXIS_TEXT = { color: '#a1a1aa' }
const AXIS_LABEL = { color: '#71717a' }
const SPLIT_LINE = { lineStyle: { color: '#27272a' } }
const TOOLTIP_BASE = {
  backgroundColor: '#18181b',
  borderColor: '#3f3f46',
  textStyle: { color: '#e4e4e7' },
}

function fmtShort(cents: number): string {
  const v = Math.abs(cents) / 100
  if (v >= 1000) return `${(v / 1000).toLocaleString('pt-BR', { maximumFractionDigits: 1 })}k`
  return v.toLocaleString('pt-BR', { maximumFractionDigits: 0 })
}

function dashboardParams(f: ItemFiltersValue) {
  return {
    date_from: f.dateFrom || undefined,
    date_to: f.dateTo || undefined,
    search: f.search || undefined,
    category_ids: f.categoryIds.length ? f.categoryIds.join(',') : undefined,
    tags: f.tagFilter.length ? f.tagFilter.join(',') : undefined,
    bank: f.bankFilter || undefined,
    kind: f.kindFilter || undefined,
    installments: f.installments === 'all' ? undefined : f.installments,
  }
}

// ---------- ECharts option builders ----------

function trendOption(data: TrendPoint[]): EChartsCoreOption {
  return {
    textStyle: AXIS_TEXT,
    tooltip: {
      trigger: 'axis',
      ...TOOLTIP_BASE,
      formatter: (ps: unknown) => {
        const v = firstAxisValue(ps)
        return axisTooltip(ps, v)
      },
    },
    legend: { top: 0, textStyle: AXIS_TEXT },
    grid: { left: 8, right: 12, top: 34, bottom: 0, containLabel: true },
    xAxis: {
      type: 'category',
      data: data.map((d) => `${d.month.slice(5, 7)}/${d.month.slice(0, 4)}`),
      axisLine: { lineStyle: { color: '#3f3f46' } },
      axisTick: { show: false },
      axisLabel: AXIS_LABEL,
    },
    yAxis: {
      type: 'value',
      axisLabel: { ...AXIS_LABEL, formatter: fmtShort },
      splitLine: SPLIT_LINE,
    },
    series: [
      {
        name: 'Despesas',
        type: 'line',
        data: data.map((d) => d.spend_cents),
        smooth: true,
        symbol: 'none',
        lineStyle: { width: 2, color: '#f87171' },
        areaStyle: { color: gradient('rgba(248,113,113,0.25)') },
      },
      {
        name: 'Receitas',
        type: 'line',
        data: data.map((d) => d.income_cents),
        smooth: true,
        symbol: 'none',
        lineStyle: { width: 2, color: '#34d399' },
        areaStyle: { color: gradient('rgba(52,211,153,0.25)') },
      },
    ],
  }
}

function pieOption(byCategory: CategoryTotal[]): EChartsCoreOption {
  return {
    textStyle: AXIS_TEXT,
    tooltip: {
      trigger: 'item',
      ...TOOLTIP_BASE,
      formatter: (p: unknown) => {
        const x = p as { name: string; value: number; percent: number }
        return `${x.name}: ${fmtCents(x.value)} (${x.percent}%)`
      },
    },
    legend: { type: 'scroll', bottom: 0, textStyle: AXIS_TEXT },
    color: PIE_COLORS,
    series: [
      {
        type: 'pie',
        radius: ['45%', '72%'],
        center: ['50%', '45%'],
        data: byCategory.map((c) => ({
          name: c.name,
          value: c.total_cents,
          itemStyle: c.color ? { color: c.color } : undefined,
        })),
        itemStyle: { borderRadius: 6, borderColor: '#09090b', borderWidth: 2 },
        label: AXIS_TEXT,
      },
    ],
  }
}

const gradient = (top: string) => ({
  type: 'linear' as const,
  x: 0,
  y: 0,
  x2: 0,
  y2: 1,
  colorStops: [
    { offset: 0, color: top },
    { offset: 1, color: 'rgba(0,0,0,0)' },
  ],
})

function axisTooltip(ps: unknown, title?: string): string {
  const arr = (Array.isArray(ps) ? ps : [ps]) as {
    marker: string
    seriesName: string
    value: number
  }[]
  let total = 0
  const lines = arr.map((p) => {
    total += p.value
    return `${p.marker} ${p.seriesName}: <b>${fmtCents(p.value)}</b>`
  })
  lines.push(`Total: <b>${fmtCents(total)}</b>`)
  const head = title ? `<div style="font-weight:600;margin-bottom:4px">${title}</div>` : ''
  return head + lines.join('<br/>')
}

function firstAxisValue(ps: unknown): string | undefined {
  const arr = (Array.isArray(ps) ? ps : [ps]) as { axisValue?: string }[]
  return arr[0]?.axisValue
}

function fmtFullDate(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}/${ymd.slice(0, 4)}`
}

function dailyOption(points: DailyPoint[], rangeDays: string[] = []): EChartsCoreOption {
  // Pivot: date → category → total.
  const byDate = new Map<string, Map<string, number>>()
  for (const p of points) {
    const key = p.key ?? 'Sem categoria'
    const m = byDate.get(p.date) ?? new Map<string, number>()
    m.set(key, (m.get(key) ?? 0) + p.total_cents)
    byDate.set(p.date, m)
  }
  // x-axis: the full range (zero days included) when the user picked one, else
  // only the days that actually have expenses.
  const dates =
    rangeDays.length > 0 ? rangeDays : [...byDate.keys()].sort()

  // Top categories by total (rest → "Outros").
  const catTotals = new Map<string, number>()
  for (const m of byDate.values()) {
    for (const [k, v] of m) catTotals.set(k, (catTotals.get(k) ?? 0) + v)
  }
  const top = [...catTotals.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 8)
    .map(([k]) => k)

  const dayValues = (d: string) => (cat: string) => byDate.get(d)?.get(cat) ?? 0
  type BarSeries = {
    name: string
    type: 'bar'
    stack: 'total'
    data: number[]
    itemStyle?: { borderRadius: number[] }
  }
  const series: BarSeries[] = top.map((cat) => ({
    name: cat,
    type: 'bar',
    stack: 'total',
    data: dates.map((d) => dayValues(d)(cat)),
  }))
  const others = dates.map((d) => {
    const mm = byDate.get(d)
    if (!mm) return 0
    let sum = 0
    for (const [k, v] of mm) if (!top.includes(k)) sum += v
    return sum
  })
  if (others.some((v) => v > 0)) {
    series.push({ name: 'Outros', type: 'bar', stack: 'total', data: others })
  }
  // Round the top of the topmost stack.
  const last = series[series.length - 1]
  if (last) last.itemStyle = { borderRadius: [4, 4, 0, 0] }

  // Scroll when the period is longer than 15 days: wheel/trackpad zoom inside
  // the plot + a slider; start showing the most recent 15 days.
  const showZoom = dates.length > 15
  const dataZoom = showZoom
    ? [
        {
          type: 'inside' as const,
          xAxisIndex: 0,
          // Wheel = pan horizontally (the natural "scroll"); drag also pans;
          // zoom stays available via the slider handles.
          moveOnMouseWheel: true,
          zoomOnMouseWheel: false,
        },
        {
          type: 'slider' as const,
          xAxisIndex: 0,
          bottom: 0,
          height: 18,
          start: Math.max(0, 100 - (15 / dates.length) * 100),
          end: 100,
          borderColor: '#27272a',
          backgroundColor: '#09090b',
          fillerColor: 'rgba(63,63,70,0.4)',
          handleStyle: { color: '#71717a' },
          moveHandleStyle: { color: '#52525b' },
          textStyle: { color: '#71717a' },
        },
      ]
    : undefined

  return {
    textStyle: AXIS_TEXT,
    tooltip: {
      trigger: 'axis',
      ...TOOLTIP_BASE,
      formatter: (ps: unknown) => {
        const v = firstAxisValue(ps)
        const title = v && v.length >= 10 ? fmtFullDate(v) : undefined
        return axisTooltip(ps, title)
      },
    },
    legend: { top: 0, type: 'scroll', textStyle: AXIS_TEXT },
    grid: {
      left: 8,
      right: 12,
      top: 34,
      bottom: showZoom ? 34 : 0,
      containLabel: true,
    },
    dataZoom,
    xAxis: {
      type: 'category',
      data: dates,
      axisLine: { lineStyle: { color: '#3f3f46' } },
      axisTick: { show: false },
      axisLabel: {
        ...AXIS_LABEL,
        formatter: (v: string) => `${v.slice(8, 10)}/${v.slice(5, 7)}`,
      },
    },
    yAxis: {
      type: 'value',
      axisLabel: { ...AXIS_LABEL, formatter: fmtShort },
      splitLine: SPLIT_LINE,
    },
    color: PIE_COLORS,
    series,
  }
}

function tagsOption(tags: TagTotal[]): EChartsCoreOption {
  const sorted = [...tags].sort((a, b) => a.total_cents - b.total_cents)
  return {
    textStyle: AXIS_TEXT,
    tooltip: {
      trigger: 'item',
      ...TOOLTIP_BASE,
      formatter: (p: unknown) => {
        const x = p as { name: string; value: number }
        return `<b>${x.name}</b><br/>${fmtCents(x.value)}`
      },
    },
    grid: { left: 8, right: 40, top: 8, bottom: 8, containLabel: true },
    xAxis: {
      type: 'value',
      axisLabel: { ...AXIS_LABEL, formatter: fmtShort },
      splitLine: SPLIT_LINE,
    },
    yAxis: {
      type: 'category',
      data: sorted.map((t) => t.tag),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: AXIS_TEXT,
    },
    series: [
      {
        name: 'Gastos',
        type: 'bar',
        data: sorted.map((t) => t.total_cents),
        barWidth: 14,
        itemStyle: { borderRadius: [0, 4, 4, 0], color: '#fbbf24' },
      },
    ],
  }
}

function calendarOption(
  days: { date: string; total_cents: number }[],
  range: [string, string],
): EChartsCoreOption {
  const max = Math.max(1, ...days.map((d) => d.total_cents))
  return {
    tooltip: {
      ...TOOLTIP_BASE,
      formatter: (p: unknown) => {
        const x = p as { data: [string, number] }
        return x.data ? `${x.data[0]}: <b>${fmtCents(x.data[1])}</b>` : ''
      },
    },
    visualMap: {
      // show: false keeps the color mapping for the heatmap cells but hides the
      // confusing gradient bar + raw-cent labels; a custom legend is rendered
      // below the chart instead.
      show: false,
      min: 0,
      max,
      calculable: false,
      orient: 'horizontal',
      left: 'center',
      bottom: 0,
      itemWidth: 12,
      itemHeight: 12,
      textStyle: AXIS_LABEL,
      inRange: { color: ['#18181b', '#78350f', '#b45309', '#d97706', '#f59e0b', '#fbbf24'] },
    },
    calendar: {
      range,
      top: 24,
      left: 56,
      right: 12,
      bottom: 34,
      cellSize: ['auto', 15],
      dayLabel: { color: '#71717a', firstDay: 0 },
      monthLabel: { color: '#a1a1aa' },
      itemStyle: { color: '#18181b', borderWidth: 2, borderColor: '#09090b' },
      splitLine: { show: false },
    },
    series: [
      {
        type: 'heatmap',
        coordinateSystem: 'calendar',
        data: days.filter((d) => d.total_cents > 0).map((d) => [d.date, d.total_cents]),
      },
    ],
  }
}

// ---------- Page ----------

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

/** First day of `monthsBack` months before `ym`, and last day of `ym`. */
function windowRange(ym: string, monthsBack: number): [string, string] {
  const [y, m] = ym.split('-').map(Number)
  const end = new Date(y, m - 1, 1)
  const start = new Date(end.getFullYear(), end.getMonth() - monthsBack, 1)
  const lastDay = new Date(end.getFullYear(), end.getMonth() + 1, 0).getDate()
  return [
    `${start.getFullYear()}-${pad(start.getMonth() + 1)}-01`,
    `${y}-${pad(m)}-${pad(lastDay)}`,
  ]
}

/** Every day (YYYY-MM-DD) in [from, to], inclusive. */
function daysInRange(from: string, to: string): string[] {
  const out: string[] = []
  const d = new Date(`${from}T00:00:00`)
  const end = new Date(`${to}T00:00:00`)
  while (d <= end) {
    out.push(`${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`)
    d.setDate(d.getDate() + 1)
  }
  return out
}

export default function Charts() {
  const location = useLocation()
  const [searchParams] = useSearchParams()
  // The monthly summary lives behind the header button (same spot where the
  // Gerar/Regenerar button is); it only renders while this is open.
  const [summaryOpen, setSummaryOpen] = useState(false)
  const { filters, setFilters } = useFiltersUrl()
  const qc = useQueryClient()

  // Pre-select the current month when landing here with an empty URL (no
  // filters at all). Any params — shared/bookmarked links, existing
  // filters — win and are left untouched.
  useEffect(() => {
    if (searchParams.size === 0) {
      const r = currentMonthRange()
      setFilters({ ...filters, dateFrom: r.from, dateTo: r.to }, { replace: true })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The digest only exists for exactly one full calendar month: the filter
  // range must start on the 1st and end on the last day of the same month.
  const isFullMonth = (from?: string, to?: string): boolean => {
    if (!from || !to) return false
    const ym = from.slice(0, 7)
    if (ym !== to.slice(0, 7) || from !== `${ym}-01`) return false
    const last = new Date(Number(ym.slice(0, 4)), Number(ym.slice(5, 7)), 0).getDate()
    return to === `${ym}-${String(last).padStart(2, '0')}`
  }
  const digestMonth = isFullMonth(filters.dateFrom, filters.dateTo) ? filters.dateFrom.slice(0, 7) : null

  // Without a single full month selected there is nothing to summarize — drop
  // the open panel (and its header button disappears on its own).
  useEffect(() => {
    if (!digestMonth) setSummaryOpen(false)
  }, [digestMonth])

  // Read the saved digest (no AI call).
  const { data: digest, isFetching: digestLoading } = useQuery({
    queryKey: ['digest', digestMonth],
    queryFn: () => (digestMonth ? dashboardApi.digestGet(digestMonth) : Promise.resolve(null)),
    enabled: !!digestMonth,
  })

  const generateDigest = useMutation({
    mutationFn: (m: string) => dashboardApi.digestGenerate(m),
    onSuccess: (d) => qc.setQueryData(['digest', d.month], d),
  })
  const deleteDigest = useMutation({
    mutationFn: (m: string) => dashboardApi.digestDelete(m),
    onSuccess: (_r, m) => qc.setQueryData(['digest', m], { ai: true, month: m, saved: false }),
  })

  // Header button: no summary yet → generate (and open the panel when done);
  // summary exists → toggle the panel.
  const toggleSummary = () => {
    if (digest?.resumo) {
      setSummaryOpen((o) => !o)
    } else if (digestMonth) {
      generateDigest.mutate(digestMonth, { onSuccess: () => setSummaryOpen(true) })
    }
  }

  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })
  const { data: allTags = [] } = useQuery({ queryKey: ['tags'], queryFn: tagsApi.list })
  const { data: banks = [] } = useQuery({ queryKey: ['banks'], queryFn: banksApi.list })

  const params = useMemo(() => dashboardParams(filters), [filters])
  // Full day list for the stacked bar (zero days included when a range is set).
  const rangeDays = useMemo(
    () => (filters.dateFrom && filters.dateTo ? daysInRange(filters.dateFrom, filters.dateTo) : []),
    [filters.dateFrom, filters.dateTo],
  )

  const { data: dash } = useQuery({
    queryKey: ['dashboard', params],
    queryFn: () => dashboardApi.get(params),
  })
  const { data: trend = [] } = useQuery({
    queryKey: ['dashboard-trend', params],
    queryFn: () => dashboardApi.trend(12, params),
  })
  const { data: daily = [] } = useQuery({
    queryKey: ['dashboard-daily', params],
    queryFn: () => dashboardApi.daily({ ...params, stack_by: 'category' }),
  })
  const { data: topTags = [] } = useQuery({
    queryKey: ['dashboard-tags', params],
    queryFn: () => dashboardApi.tags(params),
  })

  // Calendar: fixed 12-month window ending at the selected range's end month.
  const calEndYm = (filters.dateTo || currentMonth()).slice(0, 7)
  const calRange = useMemo(() => windowRange(calEndYm, 11), [calEndYm])
  const { data: calDays = [] } = useQuery({
    queryKey: ['dashboard-calendar', params, calRange],
    queryFn: () =>
      dashboardApi.daily({
        ...params,
        stack_by: 'none',
        date_from: calRange[0],
        date_to: calRange[1],
      }),
  })

  const { data: recurring = [] } = useQuery({
    queryKey: ['recurring'],
    queryFn: recurringApi.list,
  })
  // Count of active rules with a scheduled next date (forecast).
  const upcomingCount = recurring.filter((r) => r.is_active && r.next_due_on).length


  const rangeLabel = useMemo(() => {
    if (!filters.dateFrom && !filters.dateTo) {
      return 'todo o histórico'
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
        <p className="text-xs text-zinc-500">Período: {rangeLabel}</p>
        {digestMonth && (
          <span className="flex items-center gap-2">
            <button
              onClick={toggleSummary}
              disabled={digestLoading || generateDigest.isPending}
              className="rounded border border-cyan-500/50 px-2.5 py-1 text-xs font-medium text-cyan-300 hover:bg-cyan-500/10 disabled:opacity-50"
              title="Mostra/esconde o resumo mensal gerado por IA (pode ser regenerado a qualquer momento)"
            >
              {generateDigest.isPending
                ? 'Gerando…'
                : digest?.resumo
                  ? `📄 Resumo mensal ${summaryOpen ? '▾' : '▸'}`
                  : '✨ Gerar resumo (IA)'}
            </button>
          </span>
        )}
      </div>

      {digestMonth && summaryOpen && (
        <div className="mb-4 rounded border border-zinc-700 bg-zinc-900 px-4 py-3">
          {digest?.resumo ? (
            <>
              <div className="mb-2 flex flex-wrap items-baseline gap-x-2 gap-y-1">
                <h2 className="text-sm font-semibold text-zinc-200">Resumo de {digest.month}</h2>
                <span className="text-xs text-zinc-500">
                  gerado por IA · salvo em {fmtUpdatedAt(digest.updated_at)}
                </span>
                <span className="ml-auto flex items-center gap-3">
                  <button
                    onClick={() => generateDigest.mutate(digestMonth)}
                    disabled={generateDigest.isPending}
                    className="text-xs text-cyan-300 hover:text-cyan-200 disabled:opacity-50"
                    title="Regenera o resumo do mês (substitui o salvo)"
                  >
                    {generateDigest.isPending ? 'Gerando…' : '🔄 Regenerar'}
                  </button>
                  <button
                    onClick={() => deleteDigest.mutate(digestMonth)}
                    className="text-xs text-zinc-500 hover:text-red-400"
                    title="Apaga o resumo salvo deste mês"
                  >
                    Apagar
                  </button>
                </span>
              </div>
              <p className="text-sm leading-relaxed text-zinc-300">{digest.resumo}</p>
              {digest.destaques && digest.destaques.length > 0 && (
                <ul className="mt-2 list-disc space-y-0.5 pl-4 text-sm text-zinc-400">
                  {digest.destaques.map((d, i) => (
                    <li key={i}>{d}</li>
                  ))}
                </ul>
              )}
              {digest.avisos && digest.avisos.length > 0 && (
                <ul className="mt-2 list-disc space-y-0.5 pl-4 text-sm text-amber-300/90">
                  {digest.avisos.map((a, i) => (
                    <li key={i}>⚠ {a}</li>
                  ))}
                </ul>
              )}
            </>
          ) : (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-sm text-zinc-400">
                Nenhum resumo salvo para{' '}
                <span className="text-zinc-200">{digestMonth}</span>.
              </p>
              <button
                onClick={() => generateDigest.mutate(digestMonth)}
                disabled={generateDigest.isPending}
                className="rounded border border-cyan-500/50 px-2.5 py-1 text-xs font-medium text-cyan-300 hover:bg-cyan-500/10 disabled:opacity-50"
              >
                {generateDigest.isPending ? 'Gerando…' : '✨ Gerar resumo (IA)'}
              </button>
            </div>
          )}
        </div>
      )}

      <ItemFilters
        value={filters}
        onChange={setFilters}
        categories={categories}
        allTags={allTags}
        banks={banks}
        searchPlaceholder="Buscar nos gráficos…"
      />

      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <div className="flex items-center gap-2 px-4 py-3 text-sm font-medium">Gráficos</div>
        <div className="space-y-4 border-t border-zinc-800 p-4">
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
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
                <p className="text-xs text-zinc-500">Recorrentes</p>
                <p className="text-lg font-semibold tabular-nums text-cyan-400">{upcomingCount}</p>
              </div>
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Gastos por categoria</h2>
                {(dash?.by_category ?? []).length === 0 ? (
                  <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
                ) : (
                  <EChart option={pieOption(dash?.by_category ?? [])} />
                )}
              </div>

              <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">Histórico (12 meses)</h2>
                <EChart option={trendOption(trend)} />
              </div>
            </div>

            <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
              <h2 className="mb-2 text-sm font-medium text-zinc-400">
                Gastos diários por categoria <span className="font-normal text-zinc-600">(despesas)</span>
              </h2>
              {daily.length === 0 ? (
                <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
              ) : (
                <EChart option={dailyOption(daily, rangeDays)} height={280} />
              )}
            </div>

            <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
              <h2 className="mb-2 text-sm font-medium text-zinc-400">
                Calendário de gastos <span className="font-normal text-zinc-600">(despesas · 12 meses)</span>
              </h2>
              {calDays.length === 0 ? (
                <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
              ) : (
                <>
                  <EChart option={calendarOption(calDays, calRange)} height={180} />
                  <div className="mt-2 flex items-center justify-center gap-2 text-[11px] text-zinc-500">
                    <span>Menos gasto</span>
                    <div
                      className="h-2.5 w-36 rounded-full"
                      style={{
                        background:
                          'linear-gradient(to right, #18181b, #78350f, #b45309, #d97706, #f59e0b, #fbbf24)',
                      }}
                    />
                    <span>Mais gasto</span>
                  </div>
                </>
              )}
            </div>

            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
              <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
                <h2 className="mb-2 text-sm font-medium text-zinc-400">
                  Top tags <span className="font-normal text-zinc-600">(despesas)</span>
                </h2>
                {topTags.length === 0 ? (
                  <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
                ) : (
                  <EChart option={tagsOption(topTags)} height={Math.min(320, 40 + topTags.length * 28)} />
                )}
              </div>

              {(dash?.top_merchants ?? []).length > 0 && (
                <div className="rounded border border-zinc-800 bg-zinc-950 p-4">
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

            <p className="text-center text-xs text-zinc-600">
              <Link to={`/lista${location.search}`} className="hover:text-zinc-300">
                Ver todos os itens na Lista →
              </Link>
            </p>
          </div>
      </div>
    </div>
  )
}
