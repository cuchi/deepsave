export function fmtCents(cents: number, currency = 'BRL'): string {
  const value = Math.abs(cents) / 100
  const formatted = value.toLocaleString('pt-BR', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
  return currency === 'BRL' ? `R$ ${formatted}` : `${currency} ${formatted}`
}

export function currentMonth(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
}

/** First and last day (YYYY-MM-DD) of the previous calendar month. */
export function lastCompleteMonthRange(): { from: string; to: string } {
  const now = new Date()
  const last = new Date(now.getFullYear(), now.getMonth() - 1, 1)
  const lastDay = new Date(now.getFullYear(), now.getMonth(), 0).getDate()
  const pad = (n: number) => String(n).padStart(2, '0')
  return {
    from: `${last.getFullYear()}-${pad(last.getMonth() + 1)}-01`,
    to: `${last.getFullYear()}-${pad(last.getMonth() + 1)}-${pad(lastDay)}`,
  }
}

export function toIso(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

/** Range covering the last `days` days (today inclusive). */
export function daysAgoRange(days: number): { from: string; to: string } {
  const to = new Date()
  const from = new Date(to.getFullYear(), to.getMonth(), to.getDate() - (days - 1))
  return { from: toIso(from), to: toIso(to) }
}

/** First and last day of the current calendar month. */
export function currentMonthRange(): { from: string; to: string } {
  const now = new Date()
  return {
    from: toIso(new Date(now.getFullYear(), now.getMonth(), 1)),
    to: toIso(new Date(now.getFullYear(), now.getMonth() + 1, 0)),
  }
}

/** Jan 1 to Dec 31 of the current year. */
export function currentYearRange(): { from: string; to: string } {
  const y = new Date().getFullYear()
  return { from: `${y}-01-01`, to: `${y}-12-31` }
}
