import { useMemo, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { dashboardApi, recurringApi, type UpcomingItem } from '../api/client'
import { fmtCents } from '../lib/format'
import EChart, { type EChartsCoreOption } from '../components/EChart'

const HORIZONS = [30, 60, 90, 180] as const
const DEFAULT_HORIZON = 90

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

function dayShort(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}`
}

function monthTitle(ym: string): string {
  const [y, m] = ym.split('-').map(Number)
  return new Date(y, m - 1, 1).toLocaleDateString('pt-BR', { month: 'long', year: 'numeric' })
}

function forecastOption(points: { month: string; installments_cents: number; recurring_cents: number }[]): EChartsCoreOption {
  return {
    textStyle: AXIS_TEXT,
    tooltip: {
      trigger: 'axis',
      ...TOOLTIP_BASE,
      formatter: (ps: unknown) => {
        const arr = (Array.isArray(ps) ? ps : [ps]) as {
          axisValue?: string
          marker: string
          seriesName: string
          value: number
        }[]
        let total = 0
        const lines = arr.map((p) => {
          total += p.value
          return `${p.marker} ${p.seriesName}: <b>${fmtCents(p.value)}</b>`
        })
        const title = arr[0]?.axisValue ? monthTitle(arr[0].axisValue) : ''
        return `<div style="font-weight:600;margin-bottom:4px">${title}</div>${lines.join('<br/>')}<br/><b>Total: ${fmtCents(total)}</b>`
      },
    },
    legend: { top: 0, textStyle: AXIS_TEXT },
    grid: { left: 8, right: 12, top: 34, bottom: 0, containLabel: true },
    xAxis: {
      type: 'category',
      data: points.map((p) => `${p.month.slice(5, 7)}/${p.month.slice(0, 4)}`),
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
        name: 'Parcelas',
        type: 'bar',
        stack: 'total',
        data: points.map((p) => p.installments_cents),
        itemStyle: { color: '#fbbf24' },
      },
      {
        name: 'Recorrentes',
        type: 'bar',
        stack: 'total',
        data: points.map((p) => p.recurring_cents),
        itemStyle: { color: '#34d399', borderRadius: [4, 4, 0, 0] },
      },
    ],
  }
}

export default function Forecast() {
  const [horizon, setHorizon] = useState<number>(DEFAULT_HORIZON)
  const months = Math.max(1, Math.ceil(horizon / 30))

  const { data: monthlyCost } = useQuery({
    queryKey: ['recurring-monthly-cost'],
    queryFn: recurringApi.monthlyCost,
  })
  const { data: forecast = [] } = useQuery({
    queryKey: ['dashboard-forecast', months],
    queryFn: () => dashboardApi.forecast(months),
  })
  const { data: upcoming = [] } = useQuery({
    queryKey: ['dashboard-upcoming', horizon],
    queryFn: () => dashboardApi.upcoming(horizon),
  })

  const totals = useMemo(() => {
    let installments = 0
    let recurring = 0
    for (const u of upcoming) {
      if (u.kind === 'parcel') installments += u.amount_cents
      else recurring += u.amount_cents
    }
    return { installments, recurring, total: installments + recurring }
  }, [upcoming])

  // Breakdown grouped by month, with per-month subtotals.
  const byMonth = useMemo(() => {
    const map = new Map<string, UpcomingItem[]>()
    for (const u of upcoming) {
      const k = u.date.slice(0, 7)
      const list = map.get(k) ?? []
      list.push(u)
      map.set(k, list)
    }
    return [...map.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1))
  }, [upcoming])

  const monthTotals = (items: UpcomingItem[]) => {
    let parcel = 0
    let rec = 0
    for (const i of items) {
      if (i.kind === 'parcel') parcel += i.amount_cents
      else rec += i.amount_cents
    }
    return { parcel, rec, total: parcel + rec }
  }

  return (
    <div className="pb-20">
      <div className="mb-4 flex flex-wrap items-baseline gap-3">
        <h1 className="text-xl font-bold">Previsão</h1>
        <p className="text-xs text-zinc-500">Gastos esperados a partir de hoje</p>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-1">
        {HORIZONS.map((h) => (
          <button
            key={h}
            type="button"
            onClick={() => setHorizon(h)}
            className={`rounded-full px-3 py-1.5 text-xs font-medium transition-colors ${
              horizon === h
                ? 'bg-zinc-100 text-zinc-900'
                : 'text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200'
            }`}
          >
            {h} dias
          </button>
        ))}
      </div>

      <div className="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
          <p className="text-xs text-zinc-500">
            Recorrência mensal <span className="text-zinc-600">(global)</span>
          </p>
          <p className="text-lg font-semibold tabular-nums">
            {fmtCents(monthlyCost?.monthly_cents ?? 0)}
            <span className="text-xs font-normal text-zinc-500">/mês</span>
          </p>
          <p className="text-[11px] text-zinc-600">{monthlyCost?.rule_count ?? 0} regras ativas</p>
        </div>
        <div className="rounded border border-zinc-800 bg-zinc-950 p-3">
          <p className="text-xs text-zinc-500">Gastos esperados ({horizon} dias)</p>
          <p className="text-lg font-semibold tabular-nums text-amber-300">{fmtCents(totals.total)}</p>
          <p className="text-[11px] text-zinc-600">
            parcelas {fmtCents(totals.installments)} · recorrentes {fmtCents(totals.recurring)}
          </p>
        </div>
      </div>

      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <div className="border-b border-zinc-800 px-4 py-3 text-sm font-medium">
          Gastos esperados por mês
        </div>
        <div className="p-4">
          {forecast.length === 0 ? (
            <p className="py-8 text-center text-sm text-zinc-600">Sem dados</p>
          ) : (
            <EChart option={forecastOption(forecast)} height={260} />
          )}
        </div>
      </div>

      <div className="rounded border border-zinc-800 bg-zinc-900">
        <div className="flex items-center justify-between border-b border-zinc-800 px-4 py-3 text-sm font-medium">
          <span>Detalhamento</span>
          <span className="text-xs font-normal text-zinc-500">
            {upcoming.length} item{upcoming.length === 1 ? '' : 's'}
          </span>
        </div>
        <div className="divide-y divide-zinc-900">
          {byMonth.length === 0 ? (
            <p className="px-4 py-8 text-center text-sm text-zinc-600">
              Nada esperado nos próximos {horizon} dias.
            </p>
          ) : (
            byMonth.map(([ym, items]) => {
              const t = monthTotals(items)
              return (
                <div key={ym} className="px-4 py-2">
                  <div className="flex flex-wrap items-baseline gap-2 py-1 text-sm">
                    <span className="font-medium capitalize">{monthTitle(ym)}</span>
                    <span className="text-xs tabular-nums text-zinc-500">
                      total {fmtCents(t.total)} · parcelas {fmtCents(t.parcel)} · recorrentes{' '}
                      {fmtCents(t.rec)}
                    </span>
                  </div>
                  {items.map((u, i) => (
                    <div
                      key={`${u.date}-${u.kind}-${i}`}
                      className="flex items-center gap-3 border-t border-zinc-800/60 py-1.5 text-sm first:border-t-0"
                    >
                      <span className="w-10 shrink-0 text-xs tabular-nums text-zinc-500">
                        {dayShort(u.date)}
                      </span>
                      <span
                        className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] ${
                          u.kind === 'parcel'
                            ? 'bg-amber-500/15 text-amber-300'
                            : 'bg-emerald-500/15 text-emerald-300'
                        }`}
                      >
                        {u.kind === 'parcel' ? `Parcela ${u.progress ?? ''}` : 'Recorrente'}
                      </span>
                      <span className="min-w-0 flex-1 truncate" title={u.description}>
                        {u.description}
                      </span>
                      {u.category_name && (
                        <span className="shrink-0 text-xs text-zinc-500">{u.category_name}</span>
                      )}
                      <span className="shrink-0 text-sm tabular-nums">
                        {fmtCents(u.amount_cents)}
                      </span>
                    </div>
                  ))}
                </div>
              )
            })
          )}
        </div>
      </div>

      <p className="mt-3 text-center text-[11px] text-zinc-600">
        Estimativa: parcelas iguais (baseado no valor da última parcela) · detecção de
        parcelamento apenas em faturas C6 · recorrentes conforme regras ativas.
      </p>
    </div>
  )
}
