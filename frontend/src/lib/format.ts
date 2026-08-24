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
