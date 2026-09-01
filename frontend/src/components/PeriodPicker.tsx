import { useState } from 'react'
import {
  currentMonthRange,
  currentYearRange,
  daysAgoRange,
  lastCompleteMonthRange,
  lastYearRange,
  toIso,
} from '../lib/format'

interface Props {
  dateFrom: string
  dateTo: string
  onChange: (from: string, to: string, opts?: { replace?: boolean }) => void
}

const PRESETS: [string, () => { from: string; to: string }][] = [
  ['Este mês', currentMonthRange],
  ['Mês passado', lastCompleteMonthRange],
  ['Últimos 7 dias', () => daysAgoRange(7)],
  ['Últimos 30 dias', () => daysAgoRange(30)],
  ['Últimos 90 dias', () => daysAgoRange(90)],
  ['Últimos 6 meses', () => daysAgoRange(180)],
  ['Últimos 12 meses', () => daysAgoRange(365)],
  ['Este ano', currentYearRange],
  ['Ano passado', lastYearRange],
]

const WEEKDAYS = ['D', 'S', 'T', 'Q', 'Q', 'S', 'S'] // Sun-first, pt-BR
const MONTHS_SHORT = ['Jan', 'Fev', 'Mar', 'Abr', 'Mai', 'Jun', 'Jul', 'Ago', 'Set', 'Out', 'Nov', 'Dez']
const pad = (n: number) => String(n).padStart(2, '0')

function dayShort(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}`
}

function dayFull(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}/${ymd.slice(0, 4)}`
}

function monthName(y: number, m: number): string {
  return new Date(y, m, 1).toLocaleDateString('pt-BR', { month: 'long' })
}

/** Last day (YYYY-MM-DD) of the month `ym` (YYYY-MM). */
function lastDay(ym: string): string {
  const [y, m] = ym.split('-').map(Number)
  return `${ym}-${pad(new Date(y, m, 0).getDate())}`
}

function MonthGrid({
  y,
  m,
  start,
  end,
  hover,
  onDay,
  onHover,
}: {
  y: number
  m: number
  start: string
  end: string
  hover: string | null
  onDay: (d: string) => void
  onHover: (d: string | null) => void
}) {
  const firstWeekday = new Date(y, m, 1).getDay()
  const daysInMonth = new Date(y, m + 1, 0).getDate()
  const cells: (number | null)[] = [
    ...Array(firstWeekday).fill(null),
    ...Array.from({ length: daysInMonth }, (_, i) => i + 1),
  ]

  return (
    <div>
      <p className="mb-1 text-center text-xs font-medium capitalize text-zinc-300">
        {monthName(y, m)}
      </p>
      <div className="grid grid-cols-7 text-center text-[10px] text-zinc-500">
        {WEEKDAYS.map((w, i) => (
          <span key={i} className="py-0.5">
            {w}
          </span>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-0.5">
        {cells.map((day, i) => {
          if (day === null) return <span key={i} />
          const d = toIso(new Date(y, m, day))
          const isStart = d === start
          const isEnd = d === end
          const inSel = start && end && d > start && d < end
          const inHover =
            start && !end && hover && d !== start && ((d > start && d < hover) || (d < start && d > hover))
          return (
            <button
              key={i}
              type="button"
              onClick={() => onDay(d)}
              onMouseEnter={() => onHover(d)}
              onMouseLeave={() => onHover(null)}
              className={`flex h-7 items-center justify-center rounded text-[11px] transition-colors ${
                isStart || isEnd
                  ? 'bg-zinc-100 font-semibold text-zinc-900'
                  : inSel || inHover
                    ? 'bg-zinc-700/70 text-zinc-100'
                    : 'text-zinc-300 hover:bg-zinc-800'
              }`}
            >
              {day}
            </button>
          )
        })}
      </div>
    </div>
  )
}

export default function PeriodPicker({ dateFrom, dateTo, onChange }: Props) {
  const [open, setOpen] = useState(false)
  // Month-focused by default: navigating between full months is the main flow.
  const [mode, setMode] = useState<'days' | 'months'>('months')
  const [view, setView] = useState(() => {
    const base = dateFrom || toIso(new Date())
    return { y: Number(base.slice(0, 4)), m: Number(base.slice(5, 7)) - 1 }
  })
  const [year, setYear] = useState(() => view.y)
  const [start, setStart] = useState(dateFrom)
  const [end, setEnd] = useState(dateTo)
  const [hover, setHover] = useState<string | null>(null)

  const openPopup = () => {
    setStart(dateFrom)
    setEnd(dateTo)
    setHover(null)
    const base = dateFrom || toIso(new Date())
    setView({ y: Number(base.slice(0, 4)), m: Number(base.slice(5, 7)) - 1 })
    setYear(Number(base.slice(0, 4)))
    setOpen(true)
  }

  const clickDay = (d: string) => {
    if (!start || end) {
      setStart(d)
      setEnd('')
    } else if (d < start) {
      setStart(d)
      setEnd(start)
    } else {
      setEnd(d)
    }
  }

  /** YYYY-MM of the current selection when it is exactly one full calendar
   *  month (1st to last day), otherwise null. */
  const fullMonth = (): string | null => {
    if (!start || !end) return null
    const ym = start.slice(0, 7)
    if (ym !== end.slice(0, 7) || start !== `${ym}-01` || end !== lastDay(ym)) return null
    return ym
  }

  const stepMonth = (ym: string, delta: number): string => {
    const [y, m] = ym.split('-').map(Number)
    const d = new Date(y, m - 1 + delta, 1)
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`
  }

  /** Select one full calendar month and apply it immediately. */
  const applyFullMonth = (ym: string) => {
    const [y, m] = ym.split('-').map(Number)
    const from = `${ym}-01`
    const to = lastDay(ym)
    setStart(from)
    setEnd(to)
    setView({ y, m: m - 1 })
    setYear(y)
    onChange(from, to)
  }

  // Month mode is pick-one-month: a single click selects that full calendar
  // month (1st to last day) and applies it immediately. Custom ranges (incl.
  // multi-month) use the "Dias" tab.
  const clickMonth = (ym: string) => {
    applyFullMonth(ym)
  }

  const apply = () => {
    if (start && end) {
      onChange(start, end)
      setOpen(false)
    }
  }
  const clear = () => {
    onChange('', '', { replace: true })
    setOpen(false)
  }
  const preset = (from: string, to: string) => {
    onChange(from, to)
    setOpen(false)
  }
  const switchMode = (m: 'days' | 'months') => {
    setMode(m)
    setHover(null)
  }

  const label = !dateFrom && !dateTo
    ? 'Todo o histórico'
    : dateFrom && dateTo && dateFrom.slice(0, 7) === dateTo.slice(0, 7)
      ? `${dayShort(dateFrom)} a ${dayShort(dateTo)}`
      : dateFrom && dateTo
        ? `${dayFull(dateFrom)} a ${dayFull(dateTo)}`
        : dateFrom
          ? `desde ${dayFull(dateFrom)}`
          : `até ${dayFull(dateTo)}`

  const prev = () => setView((v) => (v.m === 0 ? { y: v.y - 1, m: 11 } : { y: v.y, m: v.m - 1 }))
  const next = () => setView((v) => (v.m === 11 ? { y: v.y + 1, m: 0 } : { y: v.y, m: v.m + 1 }))
  const second = view.m === 11 ? { y: view.y + 1, m: 0 } : { y: view.y, m: view.m + 1 }

  const startYm = start ? start.slice(0, 7) : ''
  const endYm = end ? end.slice(0, 7) : ''

  return (
    <div className="relative sm:col-span-2">
      <span className="mb-1 block text-[10px] font-medium uppercase tracking-wide text-zinc-500">
        Período
      </span>
      <button
        type="button"
        onClick={openPopup}
        className="field flex items-center justify-between gap-2 text-left"
      >
        <span className="truncate">{label}</span>
        <span className={`shrink-0 text-zinc-500 transition-transform ${open ? 'rotate-180' : ''}`}>
          ▾
        </span>
      </button>

      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div className="absolute left-0 top-full z-20 mt-1 w-[min(680px,calc(100vw-2rem))] max-h-[min(32rem,calc(100dvh-1rem))] overflow-y-auto rounded-lg border border-zinc-700 bg-zinc-900 p-3 shadow-2xl">
            <div className="grid gap-4 sm:grid-cols-[150px_1fr]">
              {/* Presets */}
              <div className="space-y-0.5">
                {PRESETS.map(([name, fn]) => (
                  <button
                    key={name}
                    type="button"
                    onClick={() => {
                      const r = fn()
                      preset(r.from, r.to)
                    }}
                    className="block w-full rounded px-2 py-1 text-left text-xs text-zinc-300 hover:bg-zinc-800"
                  >
                    {name}
                  </button>
                ))}
                <button
                  type="button"
                  onClick={clear}
                  className="block w-full rounded px-2 py-1 text-left text-xs text-red-400/90 hover:bg-zinc-800"
                >
                  Todo o histórico
                </button>
              </div>

              {/* Calendar */}
              <div>
                <div className="mb-2 flex items-center justify-between">
                  <div className="flex gap-1 rounded-md bg-zinc-800 p-0.5">
                    {(['days', 'months'] as const).map((m) => (
                      <button
                        key={m}
                        type="button"
                        onClick={() => switchMode(m)}
                        className={`rounded px-2 py-0.5 text-[11px] font-medium transition-colors ${
                          mode === m
                            ? 'bg-zinc-100 text-zinc-900'
                            : 'text-zinc-400 hover:text-zinc-200'
                        }`}
                      >
                        {m === 'days' ? 'Dias' : 'Meses'}
                      </button>
                    ))}
                  </div>
                  {mode === 'days' ? (
                    <div className="flex items-center">
                      <button
                        type="button"
                        onClick={prev}
                        className="px-2 text-zinc-400 hover:text-zinc-100"
                      >
                        ‹
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setYear(view.y)
                          switchMode('months')
                        }}
                        className="text-xs capitalize text-zinc-400 hover:text-zinc-200"
                        title="Selecionar por meses"
                      >
                        {monthName(view.y, view.m)} · {monthName(second.y, second.m)}
                      </button>
                      <button
                        type="button"
                        onClick={next}
                        className="px-2 text-zinc-400 hover:text-zinc-100"
                      >
                        ›
                      </button>
                    </div>
                  ) : (
                    <div className="flex items-center gap-1">
                      <button
                        type="button"
                        onClick={() => setYear((y) => y - 1)}
                        title="Ano anterior"
                        className="px-2 text-zinc-400 hover:text-zinc-100"
                      >
                        ‹
                      </button>
                      <span className="text-xs text-zinc-400">{year}</span>
                      <button
                        type="button"
                        onClick={() => setYear((y) => y + 1)}
                        title="Próximo ano"
                        className="px-2 text-zinc-400 hover:text-zinc-100"
                      >
                        ›
                      </button>
                      {fullMonth() && (
                        <>
                          <span className="mx-1 h-4 w-px bg-zinc-700" />
                          <button
                            type="button"
                            onClick={() => applyFullMonth(stepMonth(fullMonth()!, -1))}
                            title="Mês anterior"
                            className="px-2 text-zinc-400 hover:text-zinc-100"
                          >
                            ‹
                          </button>
                          <span className="text-xs capitalize text-zinc-400">
                            {monthName(
                              Number(fullMonth()!.slice(0, 4)),
                              Number(fullMonth()!.slice(5, 7)) - 1,
                            )}
                          </span>
                          <button
                            type="button"
                            onClick={() => applyFullMonth(stepMonth(fullMonth()!, 1))}
                            title="Próximo mês"
                            className="px-2 text-zinc-400 hover:text-zinc-100"
                          >
                            ›
                          </button>
                        </>
                      )}
                    </div>
                  )}
                </div>

                {mode === 'days' ? (
                  <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                    <MonthGrid
                      y={view.y}
                      m={view.m}
                      start={start}
                      end={end}
                      hover={hover}
                      onDay={clickDay}
                      onHover={setHover}
                    />
                    <MonthGrid
                      y={second.y}
                      m={second.m}
                      start={start}
                      end={end}
                      hover={hover}
                      onDay={clickDay}
                      onHover={setHover}
                    />
                  </div>
                ) : (
                  <div className="grid grid-cols-3 gap-1">
                    {MONTHS_SHORT.map((name, i) => {
                      const ym = `${year}-${pad(i + 1)}`
                      const isSel = ym === startYm && ym === endYm
                      return (
                        <button
                          key={ym}
                          type="button"
                          onClick={() => clickMonth(ym)}
                          className={`flex h-9 items-center justify-center rounded text-xs transition-colors ${
                            isSel
                              ? 'bg-zinc-100 font-semibold text-zinc-900'
                              : 'text-zinc-300 hover:bg-zinc-800'
                          }`}
                        >
                          {name}
                        </button>
                      )
                    })}
                  </div>
                )}
              </div>
            </div>

            <div className="mt-3 flex items-center justify-between border-t border-zinc-800 pt-2">
              <span className="truncate text-xs text-zinc-500">
                {start && end
                  ? `${dayFull(start)} a ${dayFull(end)}`
                  : start
                    ? `desde ${dayFull(start)}`
                    : 'Selecione o período'}
              </span>
              <span className="flex shrink-0 gap-2">
                <button
                  type="button"
                  onClick={clear}
                  className="rounded border border-zinc-700 px-2.5 py-1 text-xs text-zinc-400 hover:text-red-400"
                >
                  Limpar
                </button>
                <button
                  type="button"
                  onClick={apply}
                  disabled={!start || !end}
                  className="rounded bg-zinc-100 px-2.5 py-1 text-xs font-medium text-zinc-900 disabled:opacity-40"
                >
                  Aplicar
                </button>
              </span>
            </div>
          </div>
        </>
      )}
    </div>
  )
}
