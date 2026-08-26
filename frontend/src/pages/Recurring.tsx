import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { categoriesApi, recurringApi, type RecurringInput } from '../api/client'
import type { Category, RecurringRule } from '../lib/types'
import { fmtCents } from '../lib/format'

const FREQ_LABELS: Record<string, string> = {
  weekly: 'Semanal',
  monthly: 'Mensal',
  yearly: 'Anual',
}

const FREQ_OPTIONS: [string, string][] = [
  ['weekly', 'Semanal'],
  ['monthly', 'Mensal'],
  ['yearly', 'Anual'],
]

function windowLabel(frequency: string, interval: number): string {
  const base = FREQ_LABELS[frequency] ?? frequency
  if (interval <= 1) return base
  const unit =
    frequency === 'weekly' ? 'semanas' : frequency === 'monthly' ? 'meses' : 'anos'
  return `a cada ${interval} ${unit}`
}

/** Advance an ISO date by the window until it is >= today (preview-time math). */
function advanceDate(iso: string, frequency: string): string {
  const d = new Date(iso + 'T00:00:00')
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  let guard = 0
  while (d < today && guard++ < 1200) {
    if (frequency === 'weekly') d.setDate(d.getDate() + 7)
    else if (frequency === 'monthly') d.setMonth(d.getMonth() + 1)
    else d.setFullYear(d.getFullYear() + 1)
  }
  return d.toISOString().slice(0, 10)
}

function daysUntil(iso: string): number {
  const today = new Date()
  today.setHours(0, 0, 0, 0)
  const d = new Date(iso + 'T00:00:00')
  return Math.round((d.getTime() - today.getTime()) / 86_400_000)
}

function nextDueLabel(next: string | null, days: number | null): string {
  if (!next) return '—'
  if (days == null) return next
  if (days === 0) return `${next} (hoje)`
  if (days > 0) return `${next} (em ${days}d)`
  return next
}

// ---------- merchant autocomplete (debounced) ----------

function useMerchantSuggestions(input: string) {
  const [query, setQuery] = useState('')
  useEffect(() => {
    const t = setTimeout(() => setQuery(input.trim()), 250)
    return () => clearTimeout(t)
  }, [input])
  return useQuery({
    queryKey: ['recurring-merchants', query],
    queryFn: () => recurringApi.merchants(query),
    // Empty query → list all merchants (capped server-side at 25), so the
    // dropdown is usable right on focus.
    enabled: true,
  })
}

/**
 * Combobox for merchant names: debounced autocomplete, dropdown on focus,
 * arrow-key navigation, Enter picks the highlighted suggestion (or the typed
 * text), Escape closes.
 */
function MerchantAutocomplete({
  value,
  onChange,
  onPick,
  placeholder,
  exclude = [],
  inputClassName = '',
}: {
  value: string
  onChange: (v: string) => void
  onPick: (name: string) => void
  placeholder?: string
  /** Names already added — hidden from the suggestions. */
  exclude?: string[]
  inputClassName?: string
}) {
  const [open, setOpen] = useState(false)
  const [highlight, setHighlight] = useState(0)
  const { data = [] } = useMerchantSuggestions(value)
  const suggestions = data.filter((m) => !exclude.includes(m))

  const commit = (name: string) => {
    onPick(name)
    onChange('')
    setOpen(false)
  }

  return (
    <div className="relative">
      <input
        value={value}
        onChange={(e) => {
          onChange(e.target.value)
          setOpen(true)
          setHighlight(0)
        }}
        onFocus={() => {
          setOpen(true)
          setHighlight(0)
        }}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') {
            e.preventDefault()
            setOpen(true)
            setHighlight((h) => Math.min(h + 1, Math.max(suggestions.length - 1, 0)))
          } else if (e.key === 'ArrowUp') {
            e.preventDefault()
            setHighlight((h) => Math.max(h - 1, 0))
          } else if (e.key === 'Enter') {
            e.preventDefault()
            if (open && suggestions[highlight]) commit(suggestions[highlight])
            else if (value.trim()) commit(value.trim())
          } else if (e.key === 'Escape') {
            setOpen(false)
          }
        }}
        placeholder={placeholder}
        className={inputClassName}
      />
      {open && suggestions.length > 0 && (
        <div className="absolute left-0 right-0 top-full z-30 mt-1 max-h-48 overflow-auto rounded border border-zinc-700 bg-zinc-900 shadow-xl">
          {suggestions.map((m, i) => (
            <button
              key={m}
              type="button"
              onMouseDown={(e) => {
                e.preventDefault()
                commit(m)
              }}
              onMouseEnter={() => setHighlight(i)}
              className={`block w-full truncate px-3 py-1.5 text-left text-sm ${
                i === highlight ? 'bg-zinc-800' : ''
              }`}
            >
              {m}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// ---------- name entries editor (aliases + isolated cases) ----------

function NameEntryEditor({
  aliases,
  isolated,
  onChange,
}: {
  aliases: string[]
  isolated: string[]
  onChange: (aliases: string[], isolated: string[]) => void
}) {
  const [input, setInput] = useState('')
  const [asAlias, setAsAlias] = useState(true)

  const add = (name: string) => {
    const n = name.trim()
    if (!n) return
    if (asAlias) {
      if (!aliases.includes(n)) onChange([...aliases, n], isolated)
    } else if (!isolated.includes(n)) {
      onChange(aliases, [...isolated, n])
    }
    setInput('')
  }

  const Chip = ({ name, kind }: { name: string; kind: 'alias' | 'isolado' }) => (
    <span className="flex items-center gap-1 rounded bg-zinc-800 px-1.5 py-0.5 text-xs">
      <span title={kind === 'alias' ? 'alias (auto-match)' : 'caso isolado (não auto)'}>
        {kind === 'alias' ? '↻' : '¹'}
      </span>
      {name}
      <button
        type="button"
        onClick={() =>
          onChange(
            kind === 'alias' ? aliases.filter((x) => x !== name) : aliases,
            kind === 'isolado' ? isolated.filter((x) => x !== name) : isolated,
          )
        }
        className="text-zinc-500 hover:text-zinc-200"
      >
        ×
      </button>
    </span>
  )

  return (
    <div className="space-y-1">
      <div className="field flex flex-wrap items-center gap-1.5">
        {aliases.map((n) => (
          <Chip key={n} name={n} kind="alias" />
        ))}
        {isolated.map((n) => (
          <Chip key={n} name={n} kind="isolado" />
        ))}
        <MerchantAutocomplete
          value={input}
          onChange={setInput}
          onPick={add}
          placeholder="Nome…"
          exclude={[...aliases, ...isolated]}
          inputClassName="min-w-32 flex-1 bg-transparent text-sm outline-none"
        />
      </div>
      <label className="flex items-center gap-1.5 text-xs text-zinc-400">
        <input
          type="checkbox"
          checked={asAlias}
          onChange={(e) => setAsAlias(e.target.checked)}
        />
        usar como alias (auto-match). Desmarcar = caso isolado (não repete)
      </label>
      <p className="text-[11px] text-zinc-600">
        Nomes são validados no salvamento: devem existir nos dados e aliases não podem
        se repetir entre regras.
      </p>
    </div>
  )
}

// ---------- rule card ----------

function RuleCard({
  rule,
  categories,
  expanded,
  onToggleExpand,
  onEdit,
  onDelete,
  onToggleActive,
  preview = false,
}: {
  rule: RecurringRule
  categories: Category[]
  expanded: boolean
  onToggleExpand: () => void
  onEdit: () => void
  onDelete: () => void
  onToggleActive: () => void
  preview?: boolean
}) {
  const cat = rule.category_id ? categories.find((c) => c.id === rule.category_id) : undefined
  return (
    <div
      className={`rounded border border-zinc-800 px-3 py-2 ${
        rule.is_active ? 'bg-zinc-950' : 'bg-zinc-900 opacity-60'
      }`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <button
          onClick={onToggleExpand}
          className="shrink-0 text-xs text-zinc-500 hover:text-zinc-200"
          title="Ocorrências recentes"
        >
          {expanded ? '▾' : '▸'}
        </button>
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={rule.name}>
          {rule.name}
        </span>
        {rule.is_active || (
          <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
            pausada
          </span>
        )}
        {(cat || rule.category_name) && (
          <span className="flex shrink-0 items-center gap-1 text-xs text-zinc-400">
            {cat?.color && (
              <span className="h-2 w-2 rounded-full" style={{ background: cat.color }} />
            )}
            {cat?.name ?? rule.category_name}
          </span>
        )}
        {rule.tags.length > 0 && (
          <span className="flex shrink-0 items-center gap-1">
            {rule.tags.slice(0, 4).map((t) => (
              <span key={t} className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
                {t}
              </span>
            ))}
            {rule.tags.length > 4 && (
              <span className="text-[10px] text-zinc-600">+{rule.tags.length - 4}</span>
            )}
          </span>
        )}
        {rule.tags_conflict && (
          <span
            className="shrink-0 text-xs text-amber-400"
            title="Itens vinculados têm tags divergentes"
          >
            ⚠
          </span>
        )}
        <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
          {windowLabel(rule.frequency, rule.interval)}
        </span>
        <span className="shrink-0 tabular-nums text-sm">{fmtCents(rule.amount_cents)}</span>
        <span className="shrink-0 text-xs text-zinc-500">
          próx: {nextDueLabel(rule.next_due_on, rule.days_until)}
        </span>
        {!preview && (
          <>
            <button
              onClick={onToggleActive}
              className="shrink-0 text-xs text-zinc-500 hover:text-zinc-200"
              title={rule.is_active ? 'Pausar regra' : 'Ativar regra'}
            >
              {rule.is_active ? 'pausar' : 'ativar'}
            </button>
            <button onClick={onEdit} className="shrink-0 text-xs text-zinc-500 hover:text-zinc-200">
              editar
            </button>
            <button
              onClick={onDelete}
              className="shrink-0 text-xs text-zinc-500 hover:text-red-400"
            >
              apagar
            </button>
          </>
        )}
      </div>
      {(rule.aliases.length > 0 || rule.isolated_cases.length > 0) && (
        <div className="mt-1 flex flex-wrap items-center gap-1 pl-5 text-[11px] text-zinc-600">
          {rule.aliases.map((a) => (
            <span key={a} title="alias (auto-match)">
              ↻ {a}
            </span>
          ))}
          {rule.isolated_cases.map((a) => (
            <span key={a} title="caso isolado (não auto)">
              ¹ {a}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

// ---------- page ----------

type Draft = RecurringRule

export default function Recurring() {
  const qc = useQueryClient()
  const { data: rules = [] } = useQuery({ queryKey: ['recurring'], queryFn: recurringApi.list })
  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })

  // Add panel state.
  const [addOpen, setAddOpen] = useState(false)
  const [addName, setAddName] = useState('')
  const [addWindow, setAddWindow] = useState('yearly')
  const [addAmount, setAddAmount] = useState<number | null>(null)
  const [addCategoryId, setAddCategoryId] = useState('')
  const [addCategoryName, setAddCategoryName] = useState<string | null>(null)
  const [addLastDate, setAddLastDate] = useState<string | null>(null)
  const [addAliases, setAddAliases] = useState<string[]>([])
  const [addBusy, setAddBusy] = useState(false)
  const [addError, setAddError] = useState('')

  // Edit state: a working copy of the rule being edited.
  const [draft, setDraft] = useState<Draft | null>(null)
  const [editError, setEditError] = useState('')

  // Expandable occurrences.
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const { data: occurrences = [] } = useQuery({
    queryKey: ['recurring-occurrences', expandedId],
    queryFn: () => recurringApi.occurrences(expandedId!),
    enabled: expandedId != null,
  })

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['recurring'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const create = useMutation({
    mutationFn: recurringApi.create,
    onSuccess: () => {
      invalidate()
      resetAdd()
    },
    onError: (e: unknown) => setAddError(apiErrorMessage(e)),
  })
  const update = useMutation({
    mutationFn: (v: { id: string; input: RecurringInput }) => recurringApi.update(v.id, v.input),
    onSuccess: () => {
      invalidate()
      setDraft(null)
    },
    onError: (e: unknown) => setEditError(apiErrorMessage(e)),
  })
  const remove = useMutation({
    mutationFn: recurringApi.remove,
    onSuccess: invalidate,
  })

  const resetAdd = () => {
    setAddOpen(false)
    setAddName('')
    setAddWindow('yearly')
    setAddAmount(null)
    setAddCategoryId('')
    setAddCategoryName(null)
    setAddLastDate(null)
    setAddAliases([])
    setAddBusy(false)
    setAddError('')
  }

  const pickMerchant = async (name: string) => {
    setAddName(name)
    setAddBusy(true)
    setAddError('')
    try {
      const p = await recurringApi.merchantProfile(name)
      setAddAmount(p.amount_cents)
      setAddCategoryId(p.category_id ?? '')
      setAddCategoryName(p.category_name)
      setAddLastDate(p.last_occurred_on)
      setAddWindow(p.suggested_frequency)
      setAddAliases([name])
    } catch {
      // 404: no profile — empty defaults, strict add flow (decided).
      setAddAmount(null)
      setAddCategoryId('')
      setAddCategoryName(null)
      setAddLastDate(null)
      setAddAliases([])
    } finally {
      setAddBusy(false)
    }
  }

  const saveAdd = () => {
    const name = addName.trim()
    if (!name) {
      setAddError('Informe um nome.')
      return
    }
    setAddError('')
    const next = addLastDate ? advanceDate(addLastDate, addWindow) : null
    create.mutate({
      name,
      amount_cents: addAmount ?? 0,
      category_id: addCategoryId || null,
      frequency: addWindow,
      interval: 1,
      next_due_on: next,
      is_active: true,
      aliases: addAliases,
      isolated_cases: [],
    })
  }

  const previewRule: RecurringRule | null = useMemo(() => {
    if (!addOpen) return null
    const next = addLastDate ? advanceDate(addLastDate, addWindow) : null
    return {
      id: 'preview',
      name: addName.trim() || 'Sem nome',
      amount_cents: addAmount ?? 0,
      currency: 'BRL',
      category_id: addCategoryId || null,
      category_name: addCategoryName,
      frequency: addWindow,
      interval: 1,
      day_of_month: null,
      next_due_on: next,
      is_active: true,
      source: 'manual',
      created_at: '',
      updated_at: '',
      aliases: addAliases,
      isolated_cases: [],
      tags: [],
      tags_conflict: false,
      days_until: next ? daysUntil(next) : null,
    }
  }, [addOpen, addName, addAmount, addCategoryId, addCategoryName, addLastDate, addWindow, addAliases])

  const editRule = (r: RecurringRule) => {
    setDraft({ ...r })
    setEditError('')
    setExpandedId(null)
  }

  const saveEdit = () => {
    if (!draft) return
    if (!draft.name.trim()) {
      setEditError('Informe um nome.')
      return
    }
    setEditError('')
    update.mutate({
      id: draft.id,
      input: {
        name: draft.name.trim(),
        amount_cents: draft.amount_cents,
        category_id: draft.category_id,
        frequency: draft.frequency,
        interval: draft.interval,
        next_due_on: draft.next_due_on,
        is_active: draft.is_active,
        aliases: draft.aliases,
        isolated_cases: draft.isolated_cases,
      },
    })
  }

  const toggleActive = (r: RecurringRule) => {
    update.mutate({
      id: r.id,
      input: {
        name: r.name,
        amount_cents: r.amount_cents,
        category_id: r.category_id,
        frequency: r.frequency,
        interval: r.interval,
        next_due_on: r.next_due_on,
        is_active: !r.is_active,
        aliases: r.aliases,
        isolated_cases: r.isolated_cases,
      },
    })
  }

  const delRule = (r: RecurringRule) => {
    if (window.confirm(`Apagar a regra “${r.name}”? Os itens vinculados deixam de ser recorrentes.`)) {
      remove.mutate(r.id)
      if (expandedId === r.id) setExpandedId(null)
    }
  }

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <h1 className="text-xl font-bold">Recorrentes</h1>
        <button
          onClick={() => (addOpen ? resetAdd() : setAddOpen(true))}
          className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900"
        >
          {addOpen ? 'Cancelar' : 'Nova regra'}
        </button>
      </div>

      {/* Add panel: name + window only (strict flow), with live card preview. */}
      {addOpen && (
        <div className="mb-4 rounded border border-zinc-800 bg-zinc-900 p-3">
          <div className="flex flex-wrap items-end gap-2">
            <div className="min-w-52 flex-1">
              <label className="mb-1 block text-xs text-zinc-500">Nome</label>
              <MerchantAutocomplete
                value={addName}
                onChange={(v) => {
                  setAddName(v)
                  setAddError('')
                }}
                onPick={pickMerchant}
                placeholder="Ex.: Netflix, IPVA, PAGAMENTO VIA PIX…"
                inputClassName="field w-full"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs text-zinc-500">Janela</label>
              <select
                value={addWindow}
                onChange={(e) => setAddWindow(e.target.value)}
                className="field w-32"
              >
                {FREQ_OPTIONS.map(([v, l]) => (
                  <option key={v} value={v}>
                    {l}
                  </option>
                ))}
              </select>
            </div>
            {addBusy && <span className="pb-2 text-xs text-zinc-500">buscando perfil…</span>}
          </div>
          {addError && <p className="mt-2 text-sm text-red-400">{addError}</p>}
          <p className="mt-2 text-[11px] text-zinc-600">
            {addAliases.length > 0
              ? 'Nome encontrado nos dados: valor, categoria e primeiro alias foram preenchidos automaticamente.'
              : 'Nome desconhecido: salve a regra e complete valor/categoria depois (ou vincule itens manualmente).'}
          </p>
          <div className="mt-3">
            {previewRule && (
              <RuleCard
                rule={previewRule}
                categories={categories}
                expanded={false}
                onToggleExpand={() => {}}
                onEdit={() => {}}
                onDelete={() => {}}
                onToggleActive={() => {}}
                preview
              />
            )}
          </div>
          <div className="mt-3 flex gap-2">
            <button
              onClick={saveAdd}
              disabled={create.isPending || addBusy}
              className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-50"
            >
              Salvar regra
            </button>
            <button onClick={resetAdd} className="text-sm text-zinc-500 hover:text-zinc-200">
              cancelar
            </button>
          </div>
        </div>
      )}

      {/* Rules list */}
      {rules.length === 0 && !addOpen ? (
        <p className="text-sm text-zinc-500">
          Nenhuma regra. Crie uma para acompanhar gastos recorrentes.
        </p>
      ) : (
        <div className="space-y-2">
          {rules.map((r) =>
            draft && draft.id === r.id ? (
              <div key={r.id} className="rounded border border-zinc-700 bg-zinc-900 p-3">
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
                  <div>
                    <label className="mb-1 block text-xs text-zinc-500">Nome</label>
                    <input
                      value={draft.name}
                      onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                      className="field w-full"
                    />
                  </div>
                  <div>
                    <label className="mb-1 block text-xs text-zinc-500">Valor (R$)</label>
                    <input
                      value={Math.abs(draft.amount_cents) / 100}
                      onChange={(e) => {
                        const v = parseFloat(e.target.value.replace(',', '.'))
                        const cents = Number.isNaN(v) ? 0 : Math.round(v * 100)
                        setDraft({ ...draft, amount_cents: -cents })
                      }}
                      inputMode="decimal"
                      className="field w-full"
                    />
                  </div>
                  <div>
                    <label className="mb-1 block text-xs text-zinc-500">Categoria</label>
                    <select
                      value={draft.category_id ?? ''}
                      onChange={(e) =>
                        setDraft({ ...draft, category_id: e.target.value || null })
                      }
                      className="field w-full"
                    >
                      <option value="">Sem categoria</option>
                      {categories.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.name}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="mb-1 block text-xs text-zinc-500">Janela</label>
                    <select
                      value={draft.frequency}
                      onChange={(e) => setDraft({ ...draft, frequency: e.target.value })}
                      className="field w-full"
                    >
                      {FREQ_OPTIONS.map(([v, l]) => (
                        <option key={v} value={v}>
                          {l}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="mb-1 block text-xs text-zinc-500">Próxima data</label>
                    <input
                      type="date"
                      value={draft.next_due_on ?? ''}
                      onChange={(e) =>
                        setDraft({ ...draft, next_due_on: e.target.value || null })
                      }
                      className="field w-full"
                    />
                  </div>
                  <label className="flex items-end gap-1.5 pb-1 text-sm text-zinc-400">
                    <input
                      type="checkbox"
                      checked={draft.is_active}
                      onChange={(e) => setDraft({ ...draft, is_active: e.target.checked })}
                    />
                    ativa
                  </label>
                </div>
                <div className="mt-2">
                  <label className="mb-1 block text-xs text-zinc-500">Nomes (aliases / casos isolados)</label>
                  <NameEntryEditor
                    aliases={draft.aliases}
                    isolated={draft.isolated_cases}
                    onChange={(a, i) => setDraft({ ...draft, aliases: a, isolated_cases: i })}
                  />
                </div>
                {editError && <p className="mt-2 text-sm text-red-400">{editError}</p>}
                <div className="mt-3 flex gap-2">
                  <button
                    onClick={saveEdit}
                    disabled={update.isPending}
                    className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-50"
                  >
                    Salvar
                  </button>
                  <button
                    onClick={() => setDraft(null)}
                    className="text-sm text-zinc-500 hover:text-zinc-200"
                  >
                    cancelar
                  </button>
                </div>
              </div>
            ) : (
              <div key={r.id}>
                <RuleCard
                  rule={r}
                  categories={categories}
                  expanded={expandedId === r.id}
                  onToggleExpand={() => setExpandedId(expandedId === r.id ? null : r.id)}
                  onEdit={() => editRule(r)}
                  onDelete={() => delRule(r)}
                  onToggleActive={() => toggleActive(r)}
                />
                {expandedId === r.id && (
                  <div className="mt-0.5 rounded-b border border-t-0 border-zinc-800 bg-zinc-950 px-3 py-2">
                    <p className="mb-1 text-xs font-medium text-zinc-400">Ocorrências recentes</p>
                    {occurrences.length === 0 ? (
                      <p className="text-xs text-zinc-600">
                        Nenhuma ocorrência vinculada. Adicione aliases ou vincule itens
                        manualmente na lista.
                      </p>
                    ) : (
                      <div className="space-y-1">
                        {occurrences.map((o, i) => (
                          <div key={i} className="flex items-center gap-2 text-xs">
                            <span className="shrink-0 text-zinc-500">{o.occurred_on}</span>
                            <span className="min-w-0 flex-1 truncate text-zinc-300">
                              {o.description}
                            </span>
                            {o.linked_manually && (
                              <span className="shrink-0 text-[10px] text-zinc-600">manual</span>
                            )}
                            <span className="shrink-0 tabular-nums text-zinc-400">
                              {fmtCents(o.amount_cents)}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  )
}

function apiErrorMessage(e: unknown): string {
  if (typeof e === 'object' && e != null && 'response' in e) {
    const resp = (e as { response?: { data?: { error?: string } } }).response
    if (resp?.data?.error) return resp.data.error
  }
  return 'Falha ao salvar.'
}
