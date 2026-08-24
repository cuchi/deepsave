import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { categoriesApi, recurringApi, type RecurringInput } from '../api/client'
import type { RecurringRule, RecurringSuggestion } from '../lib/types'
import { fmtCents } from '../lib/format'

const FREQ_LABELS: Record<string, string> = {
  weekly: 'semanal',
  monthly: 'mensal',
  yearly: 'anual',
}

function nextDueFor(s: RecurringSuggestion): string {
  const d = new Date(s.last_seen)
  if (s.frequency === 'weekly') d.setDate(d.getDate() + 7 * s.interval)
  else if (s.frequency === 'yearly') d.setFullYear(d.getFullYear() + s.interval)
  else d.setMonth(d.getMonth() + s.interval)
  return d.toISOString().slice(0, 10)
}

export default function Recurring() {
  const qc = useQueryClient()
  const { data: rules = [] } = useQuery({ queryKey: ['recurring'], queryFn: recurringApi.list })
  const { data: upcoming = [] } = useQuery({ queryKey: ['recurring-upcoming'], queryFn: recurringApi.upcoming })
  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })
  const { data: suggestions = [], refetch: refetchSuggestions } = useQuery({
    queryKey: ['recurring-suggestions'],
    queryFn: recurringApi.suggestions,
    enabled: false,
  })

  const [editingId, setEditingId] = useState<string | null>(null)
  const [description, setDescription] = useState('')
  const [amount, setAmount] = useState('')
  const [frequency, setFrequency] = useState('monthly')
  const [interval, setInterval] = useState(1)
  const [categoryId, setCategoryId] = useState('')
  const [nextDueOn, setNextDueOn] = useState('')
  const [isActive, setIsActive] = useState(true)
  const [suggestionsOpen, setSuggestionsOpen] = useState(true)
  const [rulesOpen, setRulesOpen] = useState(true)

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['recurring'] })
    qc.invalidateQueries({ queryKey: ['recurring-upcoming'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const resetForm = () => {
    setEditingId(null)
    setDescription('')
    setAmount('')
    setFrequency('monthly')
    setInterval(1)
    setCategoryId('')
    setNextDueOn('')
    setIsActive(true)
  }

  const create = useMutation({
    mutationFn: recurringApi.create,
    onSuccess: () => {
      invalidate()
      resetForm()
    },
  })
  const update = useMutation({
    mutationFn: (v: { id: string; input: RecurringInput }) => recurringApi.update(v.id, v.input),
    onSuccess: () => {
      invalidate()
      resetForm()
    },
  })
  const remove = useMutation({ mutationFn: recurringApi.remove, onSuccess: invalidate })

  const editRule = (r: RecurringRule) => {
    setEditingId(r.id)
    setDescription(r.description)
    setAmount(String(Math.abs(r.amount_cents) / 100))
    setFrequency(r.frequency)
    setInterval(r.interval)
    setCategoryId(r.category_id ?? '')
    setNextDueOn(r.next_due_on ?? '')
    setIsActive(r.is_active)
  }

  const submit = (e: FormEvent) => {
    e.preventDefault()
    const parsed = parseFloat(amount.replace(',', '.'))
    if (Number.isNaN(parsed) || !description.trim()) return
    const input: RecurringInput = {
      description: description.trim(),
      amount_cents: -Math.round(parsed * 100),
      category_id: categoryId || null,
      frequency,
      interval,
      next_due_on: nextDueOn || null,
      is_active: isActive,
    }
    if (editingId) update.mutate({ id: editingId, input })
    else create.mutate(input)
  }

  const createFromSuggestion = (s: RecurringSuggestion) => {
    create.mutate({
      merchant: s.merchant,
      description: s.description,
      amount_cents: s.amount_cents,
      frequency: s.frequency,
      interval: s.interval,
      next_due_on: nextDueFor(s),
      is_active: true,
      category_id: null,
    })
  }

  const toggleActive = (r: RecurringRule) => {
    update.mutate({
      id: r.id,
      input: {
        merchant: r.merchant,
        description: r.description,
        amount_cents: r.amount_cents,
        category_id: r.category_id,
        frequency: r.frequency,
        interval: r.interval,
        next_due_on: r.next_due_on,
        is_active: !r.is_active,
      },
    })
  }

  return (
    <div>
      <h1 className="mb-4 text-xl font-bold">Recorrentes</h1>

      {/* Suggestions */}
      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setSuggestionsOpen(!suggestionsOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Sugestões
          <span className="ml-auto text-zinc-500">{suggestionsOpen ? '▾' : '▸'}</span>
        </button>
        {suggestionsOpen && (
          <div className="border-t border-zinc-800 p-3">
            <button
              onClick={() => refetchSuggestions()}
              className="mb-3 rounded bg-zinc-100 px-3 py-1.5 text-xs font-medium text-zinc-900"
            >
              Sugerir recorrências
            </button>
            {suggestions.length === 0 ? (
              <p className="text-sm text-zinc-500">Nenhuma sugestão (clique em “Sugerir”).</p>
            ) : (
              <div className="space-y-2">
                {suggestions.map((s, i) => (
                  <div
                    key={i}
                    className="flex items-center gap-3 rounded border border-zinc-800 bg-zinc-950 px-3 py-2 text-sm"
                  >
                    <span className="min-w-0 flex-1 truncate">{s.description}</span>
                    <span className="text-zinc-500">{FREQ_LABELS[s.frequency]}</span>
                    <span className="tabular-nums">{fmtCents(s.amount_cents)}</span>
                    <span className="text-xs text-zinc-500">×{s.count}</span>
                    <button
                      onClick={() => createFromSuggestion(s)}
                      className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                    >
                      criar
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Rules */}
      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setRulesOpen(!rulesOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Regras
          <span className="ml-auto text-zinc-500">{rulesOpen ? '▾' : '▸'}</span>
        </button>
        {rulesOpen && (
          <div className="border-t border-zinc-800 p-3">
            <form onSubmit={submit} className="mb-4 flex flex-wrap gap-2">
              <input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Nome"
                className="field w-44"
              />
              <input
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                placeholder="Valor (R$)"
                inputMode="decimal"
                className="field w-28"
              />
              <select value={frequency} onChange={(e) => setFrequency(e.target.value)} className="field w-28">
                <option value="weekly">Semanal</option>
                <option value="monthly">Mensal</option>
                <option value="yearly">Anual</option>
              </select>
              <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} className="field w-40">
                <option value="">Sem categoria</option>
                {categories.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
              <input
                type="date"
                value={nextDueOn}
                onChange={(e) => setNextDueOn(e.target.value)}
                className="field w-40"
              />
              <button className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900">
                {editingId ? 'Salvar' : 'Adicionar'}
              </button>
              {editingId && (
                <button type="button" onClick={resetForm} className="text-sm text-zinc-500 hover:text-zinc-200">
                  cancelar
                </button>
              )}
            </form>

            {rules.length === 0 ? (
              <p className="text-sm text-zinc-500">Nenhuma regra.</p>
            ) : (
              <div className="space-y-2">
                {rules.map((r) => (
                  <div
                    key={r.id}
                    className={`flex items-center gap-3 rounded border border-zinc-800 px-3 py-2 text-sm ${r.is_active ? 'bg-zinc-950' : 'bg-zinc-900 opacity-60'}`}
                  >
                    <span className="min-w-0 flex-1 truncate">{r.description}</span>
                    <span className="text-zinc-500">{FREQ_LABELS[r.frequency]}</span>
                    <span className="tabular-nums">{fmtCents(r.amount_cents)}</span>
                    {r.next_due_on && <span className="text-xs text-zinc-500">próx: {r.next_due_on}</span>}
                    <button
                      onClick={() => toggleActive(r)}
                      className="text-xs text-zinc-500 hover:text-zinc-200"
                    >
                      {r.is_active ? 'pausar' : 'ativar'}
                    </button>
                    <button onClick={() => editRule(r)} className="text-xs text-zinc-500 hover:text-zinc-200">
                      editar
                    </button>
                    <button onClick={() => remove.mutate(r.id)} className="text-xs text-zinc-500 hover:text-red-400">
                      apagar
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Upcoming */}
      <div className="rounded border border-zinc-800 bg-zinc-900">
        <h2 className="px-4 py-3 text-sm font-medium">Próximas ocorrências</h2>
        <div className="border-t border-zinc-800 p-3">
          {upcoming.length === 0 ? (
            <p className="text-sm text-zinc-500">Nada previsto.</p>
          ) : (
            <div className="space-y-1">
              {upcoming.map((u) => (
                <div key={u.id + u.next_due_on} className="flex items-center gap-3 text-sm">
                  <span className="min-w-0 flex-1 truncate">{u.description}</span>
                  <span className="text-xs text-zinc-500">
                    {u.next_due_on}
                    {u.days_until >= 0 ? ` (em ${u.days_until}d)` : ` (há ${-u.days_until}d)`}
                  </span>
                  <span className="tabular-nums">{fmtCents(u.amount_cents)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
