import { useEffect, useState } from 'react'
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
    // The picker's own onChange handles clearing when needed (e.g. NameEntryEditor
    // clears its input after adding a chip); here we only report the pick so the
    // create-flow name field keeps the picked value.
    onPick(name)
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
  onChipPick,
}: {
  aliases: string[]
  isolated: string[]
  onChange: (aliases: string[], isolated: string[]) => void
  /** Clicking a chip prefills the form values from that name's merchant profile. */
  onChipPick?: (name: string) => void
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
      <span
        className="cursor-pointer hover:text-zinc-100"
        title="Clique para preencher valor, janela e próxima data a partir deste nome"
        onClick={() => onChipPick?.(name)}
      >
        {name}
      </span>
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
        se repetir entre regras. Um alias pode ignorar o valor no final do nome —
        ex.: “netflix” identifica “NETFLIX.COM 22,90”.
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
}: {
  rule: RecurringRule
  categories: Category[]
  expanded: boolean
  onToggleExpand: () => void
  onEdit: () => void
  onDelete: () => void
  onToggleActive: () => void
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
        <span className="flex shrink-0 items-center gap-2">
          <button
            onClick={onToggleActive}
            className="text-xs text-zinc-500 hover:text-zinc-200"
            title={rule.is_active ? 'Pausar regra' : 'Ativar regra'}
          >
            {rule.is_active ? 'pausar' : 'ativar'}
          </button>
          <button onClick={onEdit} className="text-xs text-zinc-500 hover:text-zinc-200">
            editar
          </button>
          <button
            onClick={onDelete}
            className="text-xs text-zinc-500 hover:text-red-400"
          >
            apagar
          </button>
        </span>
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

// ---------- shared create/edit form ----------

function RuleForm({
  draft,
  onChange,
  busy,
  error,
  saveLabel,
  onSave,
  onCancel,
}: {
  draft: RuleDraft
  onChange: (d: RuleDraft) => void
  busy: boolean
  error: string
  saveLabel: string
  onSave: () => void
  onCancel: () => void
}) {
  const set = (patch: Partial<RuleDraft>) => onChange({ ...draft, ...patch })

  /** Alias chip clicked → prefill amount/window/next date from that name. */
  const pickAliasValues = async (name: string) => {
    try {
      const p = await recurringApi.merchantProfile(name)
      onChange({
        ...draft,
        amount_cents: p.amount_cents,
        frequency: p.suggested_frequency,
        interval: p.suggested_interval ?? 1,
        next_due_on: p.next_due_on ?? null,
      })
    } catch {
      // Name without a profile (no items in the window) — nothing to prefill.
    }
  }

  return (
    <div className="rounded border border-zinc-700 bg-zinc-900 p-3">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
        <div>
          <label className="mb-1 block text-xs text-zinc-500">Nome</label>
          <input
            value={draft.name}
            onChange={(e) => set({ name: e.target.value })}
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
              set({ amount_cents: -cents })
            }}
            inputMode="decimal"
            className="field w-full"
          />
        </div>
        <div>
          <label className="mb-1 block text-xs text-zinc-500">Janela</label>
          <select
            value={draft.frequency}
            onChange={(e) => set({ frequency: e.target.value })}
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
            onChange={(e) => set({ next_due_on: e.target.value || null })}
            className="field w-full"
          />
        </div>
        <label className="flex items-end gap-1.5 pb-1 text-sm text-zinc-400">
          <input
            type="checkbox"
            checked={draft.is_active}
            onChange={(e) => set({ is_active: e.target.checked })}
          />
          ativa
        </label>
      </div>
      <div className="mt-2">
        <label className="mb-1 block text-xs text-zinc-500">Nomes (aliases / casos isolados)</label>
        <NameEntryEditor
          aliases={draft.aliases}
          isolated={draft.isolated_cases}
          onChange={(a, i) => set({ aliases: a, isolated_cases: i })}
          onChipPick={pickAliasValues}
        />
      </div>
      {error && <p className="mt-2 text-sm text-red-400">{error}</p>}
      <div className="mt-3 flex gap-2">
        <button
          onClick={onSave}
          disabled={busy}
          className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-50"
        >
          {saveLabel}
        </button>
        <button onClick={onCancel} className="text-sm text-zinc-500 hover:text-zinc-200">
          cancelar
        </button>
      </div>
    </div>
  )
}

// ---------- page ----------

/** Editable subset shared by the create and edit forms. */
type RuleDraft = {
  name: string
  amount_cents: number
  frequency: string
  interval: number
  next_due_on: string | null
  is_active: boolean
  aliases: string[]
  isolated_cases: string[]
}

function emptyDraft(): RuleDraft {
  return {
    name: '',
    amount_cents: 0,
    frequency: 'yearly',
    interval: 1,
    next_due_on: null,
    is_active: true,
    aliases: [],
    isolated_cases: [],
  }
}

export default function Recurring() {
  const qc = useQueryClient()
  const { data: rules = [] } = useQuery({ queryKey: ['recurring'], queryFn: recurringApi.list })
  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })

  // Create panel state: `null` = closed.
  const [addDraft, setAddDraft] = useState<RuleDraft | null>(null)
  const [addError, setAddError] = useState('')

  // Edit state: a working copy of the rule being edited.
  const [draft, setDraft] = useState<RecurringRule | null>(null)
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
    setAddDraft(null)
    setAddError('')
  }

  const saveAdd = () => {
    if (!addDraft) return
    if (!addDraft.name.trim()) {
      setAddError('Informe um nome.')
      return
    }
    setAddError('')
    create.mutate({
      name: addDraft.name.trim(),
      amount_cents: addDraft.amount_cents,
      frequency: addDraft.frequency,
      interval: addDraft.interval,
      next_due_on: addDraft.next_due_on,
      is_active: addDraft.is_active,
      aliases: addDraft.aliases,
      isolated_cases: addDraft.isolated_cases,
    })
  }

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
          onClick={() => (addDraft ? resetAdd() : setAddDraft(emptyDraft()))}
          className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900"
        >
          {addDraft ? 'Cancelar' : 'Nova regra'}
        </button>
      </div>

      {/* Create panel: same full form as edit (incl. aliases editor). */}
      {addDraft && (
        <div className="mb-4">
          <RuleForm
            draft={addDraft}
            onChange={setAddDraft}
            busy={create.isPending}
            error={addError}
            saveLabel="Salvar regra"
            onSave={saveAdd}
            onCancel={resetAdd}
          />
        </div>
      )}

      {/* Rules list */}
      {rules.length === 0 && !addDraft ? (
        <p className="text-sm text-zinc-500">
          Nenhuma regra. Crie uma para acompanhar gastos recorrentes.
        </p>
      ) : (
        <div className="space-y-2">
          {rules.map((r) =>
            draft && draft.id === r.id ? (
              <RuleForm
                key={r.id}
                draft={draft}
                onChange={(d) => setDraft({ ...draft, ...d })}
                busy={update.isPending}
                error={editError}
                saveLabel="Salvar"
                onSave={saveEdit}
                onCancel={() => setDraft(null)}
              />
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
