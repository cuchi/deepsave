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
