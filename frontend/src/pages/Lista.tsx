import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  aiTagsApi,
  banksApi,
  categoriesApi,
  itemsApi,
  recurringApi,
  tagsApi,
  type BulkItemUpdateInput,
} from '../api/client'
import type { AiTagBatch, Item, RecurringRule, SuggestionDetail } from '../lib/types'
import { currentMonth, fmtCents } from '../lib/format'
import ItemForm from '../components/ItemForm'
import SuggestionReviewModal from '../components/SuggestionReviewModal'
import BulkEditModal from '../components/BulkEditModal'
import BankLogo from '../components/BankLogo'
import ItemFilters, { useFiltersUrl } from '../components/ItemFilters'

/** Global list cap: whole history, most recent first, capped for bulk workflows. */
const LIST_LIMIT = 500

interface FormState {
  open: boolean
  editing?: Item | null
}

const KIND_LABELS: Record<string, string> = {
  income: 'Receita',
  refund: 'Estorno',
  internal: 'Interna',
}

function shortDate(ymd: string): string {
  return `${ymd.slice(8, 10)}/${ymd.slice(5, 7)}/${ymd.slice(0, 4)}`
}

function itemTitle(it: Item): string {
  if (it.merchant) return it.merchant
  const parts = it.description.split(' - ')
  const idx = (parts[0] ?? '').toLowerCase().includes('estorno') ? 2 : 1
  const candidate = parts[idx]?.trim()
  if (candidate && candidate.replace(/\D/g, '').length !== 14) {
    return candidate
  }
  return it.description
}

function amountColor(cents: number): string {
  if (cents < 0) return 'text-red-400'
  if (cents > 0) return 'text-emerald-400'
  return 'text-zinc-400'
}

function signOf(cents: number): string {
  return cents > 0 ? '+' : cents < 0 ? '−' : ''
}

export default function Lista() {
  const navigate = useNavigate()
  const [form, setForm] = useState<FormState>({ open: false })
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [bulkOpen, setBulkOpen] = useState(false)
  const { filters, setFilters } = useFiltersUrl()
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [detailsFor, setDetailsFor] = useState<string | null>(null)
  const [linkFor, setLinkFor] = useState<string | null>(null)
  const [linkQuery, setLinkQuery] = useState('')
  const [listOpen, setListOpen] = useState(true)
  const qc = useQueryClient()

  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })
  const { data: banks = [] } = useQuery({ queryKey: ['banks'], queryFn: banksApi.list })
  const { data: allTags = [] } = useQuery({ queryKey: ['tags'], queryFn: tagsApi.list })
  const { data: tagRegistry = [] } = useQuery({ queryKey: ['tags-registry'], queryFn: tagsApi.registry })
  const tagDesc = new Map(tagRegistry.map((r) => [r.name, r.description]))
  const { data: rules = [] } = useQuery({ queryKey: ['recurring'], queryFn: recurringApi.list })

  const { data: items = [], isLoading, isPlaceholderData } = useQuery({
    queryKey: ['items', 'all', filters],
    queryFn: () =>
      itemsApi.list({
        search: filters.search || undefined,
        category_ids: filters.categoryIds.length ? filters.categoryIds.join(',') : undefined,
        tags: filters.tagFilter.length ? filters.tagFilter.join(',') : undefined,
        bank: filters.bankFilter || undefined,
        kind: filters.kindFilter || undefined,
        sort: filters.sortBy || undefined,
        limit: LIST_LIMIT,
        installments: filters.installments === 'all' ? undefined : filters.installments,
        date_from: filters.dateFrom || undefined,
        date_to: filters.dateTo || undefined,
      }),
    // Keep the previous list rendered while a filter change refetches, so the
    // page doesn't collapse to a loading line and the scroll position holds.
    placeholderData: (prev) => prev,
  })

  // Net total of everything matching the filters (root items only, no cap).
  const { data: summary = null } = useQuery({
    queryKey: ['items-summary', filters],
    queryFn: () =>
      itemsApi.summary({
        search: filters.search || undefined,
        category_ids: filters.categoryIds.length ? filters.categoryIds.join(',') : undefined,
        tags: filters.tagFilter.length ? filters.tagFilter.join(',') : undefined,
        bank: filters.bankFilter || undefined,
        kind: filters.kindFilter || undefined,
        installments: filters.installments === 'all' ? undefined : filters.installments,
        date_from: filters.dateFrom || undefined,
        date_to: filters.dateTo || undefined,
      }),
  })

  const del = useMutation({
    mutationFn: (id: string) => itemsApi.remove(id),
    onSuccess: (_data, id) => {
      qc.invalidateQueries({ queryKey: ['items'] })
      // Drop a deleted item from the selection so the count stays accurate.
      setSelected((prev) => {
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    },
  })

  const linkRecurring = useMutation({
    mutationFn: (v: { id: string; ruleId: string | null }) =>
      itemsApi.linkRecurring(v.id, v.ruleId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['items'] })
      qc.invalidateQueries({ queryKey: ['recurring'] })
      setLinkFor(null)
    },
  })

  const openLinkPicker = (id: string) => {
    setMenuFor(null)
    setLinkQuery('')
    setLinkFor(id)
  }

  const bulk = useMutation({
    mutationFn: (input: BulkItemUpdateInput) => itemsApi.bulkUpdate(input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['items'] })
      qc.invalidateQueries({ queryKey: ['tags'] })
      qc.invalidateQueries({ queryKey: ['memory'] })
      setBulkOpen(false)
      setSelected(new Set())
    },
  })

  // AI bulk tagging: enqueue a background batch; proposals surface inline
  // (banner + per-row ✨ chip) as soon as the worker finishes.
  const [aiTagError, setAiTagError] = useState('')
  const tagWithAi = useMutation({
    mutationFn: (v: { ids: string[]; kind: 'tags' | 'categorize' | 'full' }) =>
      aiTagsApi.createBatch(v.ids, v.kind),
    onSuccess: () => {
      setSelected(new Set())
      setAiTagError('')
      refetchAi()
      qc.invalidateQueries({ queryKey: ['ai-batches'] })
    },
    onError: (e) => {
      const msg = (e as { response?: { data?: { error?: string } } })?.response?.data?.error
      setAiTagError(msg ?? 'falha ao enfileirar tags')
    },
  })

  // Pending tag suggestions (poll while any remain).
  const [reviewOpen, setReviewOpen] = useState(false)
  const [reviewStartId, setReviewStartId] = useState<string | null>(null)
  const { data: aiSuggestions = [], refetch: refetchAi } = useQuery({
    queryKey: ['ai-suggestions'],
    queryFn: () => aiTagsApi.listSuggestions(),
    refetchInterval: (query) => {
      // Poll while there are suggestions to review OR the worker is still
      // running a batch (a batch can take minutes — suggestions appear only
      // when it finishes).
      const batches = qc.getQueryData<AiTagBatch[]>(['ai-batches'])
      const hasWork =
        (query.state.data?.length ?? 0) > 0 ||
        (batches ?? []).some((b) => b.status === 'pending' || b.status === 'processing')
      return hasWork ? 6000 : false
    },
  })
  const { data: aiBatches = [] } = useQuery({
    queryKey: ['ai-batches'],
    queryFn: () => aiTagsApi.listBatches(),
    refetchInterval: (query) => {
      const active = (query.state.data ?? []).some(
        (b) => b.status === 'pending' || b.status === 'processing',
      )
      return active ? 6000 : false
    },
  })
  const aiWorking = aiBatches.some((b) => b.status === 'pending' || b.status === 'processing')
  const aiWorkCount =
    aiBatches.find((b) => b.status === 'pending' || b.status === 'processing')?.item_count ?? 0
  // Worker failures happen async (the enqueue POST succeeds) — surface them.
  const failedBatch = aiBatches.find(
    (b) =>
      b.status === 'failed' &&
      Date.now() - new Date(b.created_at).getTime() < 60 * 60 * 1000,
  )
  const doneSuggestions = aiSuggestions.filter((s) => s.batch_status === 'done')
  const sugByItem = new Map(doneSuggestions.map((s) => [s.item_id, s]))
  const [suggestionsOnly, setSuggestionsOnly] = useState(false)
  const visibleItems = suggestionsOnly ? items.filter((i) => sugByItem.has(i.id)) : items

  const refreshAi = () => {
    refetchAi()
    qc.invalidateQueries({ queryKey: ['tags-registry'] })
    qc.invalidateQueries({ queryKey: ['tags'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }
  const applySug = useMutation({
    mutationFn: (v: { id: string; tags?: string[]; category?: string }) =>
      aiTagsApi.apply(v.id, { tags: v.tags, category: v.category }),
    onSuccess: () => refreshAi(),
  })
  const dismissSug = useMutation({
    mutationFn: aiTagsApi.dismiss,
    onSuccess: () => refreshAi(),
  })
  const applyAllSug = useMutation({ mutationFn: () => aiTagsApi.applyAll(), onSuccess: refreshAi })
  const dismissAllSug = useMutation({ mutationFn: () => aiTagsApi.dismissAll(), onSuccess: refreshAi })

  const openSuggestion = (s: SuggestionDetail) => {
    setReviewStartId(s.id)
    setReviewOpen(true)
  }

  // Selection is scoped to what's currently visible: changing any filter resets
  // it (avoids accidentally bulk-editing hidden items).
  useEffect(() => {
    setSelected(new Set())
  }, [
    filters.search,
    filters.categoryIds,
    filters.tagFilter,
    filters.bankFilter,
    filters.kindFilter,
    filters.installments,
    filters.dateFrom,
    filters.dateTo,
  ])

  const catById = new Map(categories.map((c) => [c.id, c]))
  const bankByDoc = new Map<string, string | undefined>()

  const roots = visibleItems
  const rootIds = roots.map((r) => r.id)
  const allSelected = roots.length > 0 && roots.every((r) => selected.has(r.id))
  const totalLabel = summary
    ? `${summary.total_cents > 0 ? '+' : summary.total_cents < 0 ? '−' : ''}${fmtCents(summary.total_cents)}`
    : '—'

  const renderRoot = (it: Item) => {
    const cat = it.category_id ? catById.get(it.category_id) : undefined
    const bank = it.bank ?? (it.document_id ? bankByDoc.get(it.document_id) : undefined)
    const kindLabel = KIND_LABELS[it.kind]
    const open = menuFor === it.id
    const detailsOpen = detailsFor === it.id
    const legacy = !it.external_id && it.source !== 'pluggy'
    const sug = sugByItem.get(it.id)

    return (
      <div key={it.id}>
        <div
          className={`group relative flex flex-wrap items-center gap-x-2 gap-y-1 py-1.5 ${
            legacy ? 'rounded border-l-2 border-amber-500 bg-amber-500/15' : ''
          }`}
        >
          <input
            type="checkbox"
            checked={selected.has(it.id)}
            onChange={(e) => {
              const next = new Set(selected)
              if (e.target.checked) {
                next.add(it.id)
              } else {
                next.delete(it.id)
              }
              setSelected(next)
            }}
            onClick={(e) => e.stopPropagation()}
            title="Selecionar para edição em massa"
            className="checkbox shrink-0"
          />
          <button
            onClick={() => setDetailsFor(detailsOpen ? null : it.id)}
            aria-label={detailsOpen ? 'Recolher detalhes' : 'Expandir detalhes'}
            className="shrink-0 px-1 py-1 text-xs text-zinc-500 hover:text-zinc-200"
          >
            {detailsOpen ? '▾' : '▸'}
          </button>
          <BankLogo bank={bank} />
          <span className="w-[4.6rem] shrink-0 text-xs tabular-nums text-zinc-500">
            {shortDate(it.occurred_on)}
          </span>
          {legacy && (
            <span className="shrink-0 rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-medium text-amber-300">
              legado
            </span>
          )}
          <button
            onClick={() => setDetailsFor(detailsOpen ? null : it.id)}
            className="min-w-0 flex-1 truncate text-left text-sm hover:text-zinc-200"
            title={`${it.description} — tocar para ver detalhes`}
          >
            {itemTitle(it)}
          </button>
          {it.installment_count != null && (
            <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
              {it.installment ?? '?'}/{it.installment_count}
            </span>
          )}
          {kindLabel && (
            <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
              {kindLabel}
            </span>
          )}
          {it.kind === 'refund' && it.refunded_item_id && (
            <span className="shrink-0 rounded bg-emerald-950 px-1.5 py-0.5 text-[10px] font-medium text-emerald-300">
              ↔ reembolso
            </span>
          )}
          {it.recurring_id && (
            <button
              onClick={() => navigate('/recurring')}
              className="shrink-0 rounded bg-sky-950 px-1.5 py-0.5 text-[10px] text-sky-300 hover:bg-sky-900"
              title="Vinculado a uma regra recorrente (abrir Recorrentes)"
            >
              ↻ recorrente
            </button>
          )}
          {cat && (
            <span className="flex shrink-0 items-center gap-1 text-xs text-zinc-400">
              {cat.color && <span className="h-2 w-2 rounded-full" style={{ background: cat.color }} />}
              {cat.name}
            </span>
          )}
          {sug && (sug.batch_kind === 'categorize' || sug.batch_kind === 'full') && sug.suggested_category !== '' && (
            <button
              onClick={() => openSuggestion(sug)}
              title="Categoria sugerida pela IA — toque para aplicar/ignorar"
              className="rounded border border-amber-500/50 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300 hover:bg-amber-500/20"
            >
              {sug.suggested_category}
            </button>
          )}
          {sug && sug.batch_kind !== 'categorize' && sug.suggested_tags.length > 0 && (
            <span className="flex shrink-0 items-center gap-1">
              {sug.suggested_tags.slice(0, 3).map((t) => (
                <button
                  key={t}
                  onClick={() => openSuggestion(sug)}
                  title="Sugestão da IA — toque para aplicar/ignorar"
                  className="rounded border border-amber-500/50 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300 hover:bg-amber-500/20"
                >
                  {t}
                </button>
              ))}
            </span>
          )}
          {it.tags.length > 0 && (
            <span className="flex shrink-0 items-center gap-1">
              {it.tags.slice(0, 2).map((t) => (
                <span
                  key={t}
                  title={tagDesc.get(t)}
                  className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400"
                >
                  {t}
                </span>
              ))}
              {it.tags.length > 2 && (
                <span className="text-[10px] text-zinc-600">+{it.tags.length - 2}</span>
              )}
            </span>
          )}
          <span className={`shrink-0 text-sm tabular-nums ${amountColor(it.amount_cents)}`}>
            {signOf(it.amount_cents)}
            {fmtCents(it.amount_cents)}
          </span>
          {sug &&
            (sug.batch_kind === 'categorize'
              ? sug.suggested_category === ''
              : sug.suggested_tags.length === 0) && (
              <button
                onClick={() => openSuggestion(sug)}
                aria-label="Revisar sugestão da IA"
                title="Sugestão da IA (vazia — toque para revisar)"
                className="shrink-0 px-1.5 py-1 text-sm text-amber-400 hover:text-amber-200"
              >
                ✨
              </button>
            )}
          <button
            onClick={() => setMenuFor(open ? null : it.id)}
            aria-label="Ações do item"
            className="shrink-0 px-2 py-1 text-zinc-500 hover:text-zinc-200"
          >
            ⋯
          </button>

          {open && (
            <>
              <div className="fixed inset-0 z-10" onClick={() => setMenuFor(null)} />
              <div className="absolute right-0 top-full z-20 mt-1 w-44 rounded border border-zinc-700 bg-zinc-900 py-1 shadow-xl">
                <button
                  onClick={() => {
                    setMenuFor(null)
                    setForm({ open: true, editing: it })
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                >
                  Editar
                </button>
                <button
                  onClick={() => openLinkPicker(it.id)}
                  className="block w-full px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                >
                  {it.recurring_id ? 'Trocar regra recorrente…' : 'Vincular a regra…'}
                </button>
                {it.recurring_id && (
                  <button
                    onClick={() => {
                      setMenuFor(null)
                      linkRecurring.mutate({ id: it.id, ruleId: null })
                    }}
                    className="block w-full px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                  >
                    Desvincular da regra
                  </button>
                )}
                <button
                  onClick={() => {
                    setMenuFor(null)
                    del.mutate(it.id)
                  }}
                  className="block w-full px-3 py-1.5 text-left text-sm text-red-400 hover:bg-zinc-800"
                >
                  Apagar
                </button>
              </div>
            </>
          )}

          {linkFor === it.id && (
            <>
              <div className="fixed inset-0 z-10" onClick={() => setLinkFor(null)} />
              <div className="absolute right-0 top-full z-20 mt-1 w-64 rounded border border-zinc-700 bg-zinc-900 py-1 shadow-xl">
                <input
                  value={linkQuery}
                  onChange={(e) => setLinkQuery(e.target.value)}
                  placeholder="Buscar regra…"
                  autoFocus
                  className="mx-2 mb-1 w-[calc(100%-16px)] rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm outline-none focus:border-zinc-500"
                />
                <button
                  onClick={() => linkRecurring.mutate({ id: it.id, ruleId: null })}
                  className="block w-full px-3 py-1.5 text-left text-sm text-zinc-500 hover:bg-zinc-800"
                >
                  Sem regra (desvincular)
                </button>
                {rules
                  .filter((r) => r.name.toLowerCase().includes(linkQuery.toLowerCase()))
                  .map((r: RecurringRule) => (
                    <button
                      key={r.id}
                      onClick={() => linkRecurring.mutate({ id: it.id, ruleId: r.id })}
                      className="block w-full truncate px-3 py-1.5 text-left text-sm hover:bg-zinc-800"
                    >
                      {r.name}
                    </button>
                  ))}
              </div>
            </>
          )}
        </div>

        {detailsOpen && (
          <div className="mb-1 rounded border border-zinc-800 bg-zinc-900/60 px-3 py-2 text-xs text-zinc-400">
            <p className="mb-1 whitespace-pre-wrap text-zinc-300">{it.description}</p>
            {it.merchant && <p>Comerciante: {it.merchant}</p>}
            {it.tags.length > 0 && (
              <p className="mb-1 flex flex-wrap items-center gap-1">
                <span>Tags:</span>
                {it.tags.map((t) => (
                  <span
                    key={t}
                    title={tagDesc.get(t)}
                    className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400"
                  >
                    {t}
                  </span>
                ))}
              </p>
            )}
            {it.installment_count != null && (
              <p>
                Parcela: {it.installment}/{it.installment_count}
              </p>
            )}
            <p>
              Fonte: {it.source_label ?? it.source} · Tipo: {it.kind}
            </p>
          </div>
        )}
      </div>
    )
  }

  const formMonth = form.editing
    ? form.editing.occurred_on.slice(0, 7)
    : currentMonth()

  return (
    <div className="pb-20">
      <div className="mb-4 flex items-baseline gap-3">
        <h1 className="text-xl font-bold">Lista</h1>
        <p className="text-xs text-zinc-500">
          {suggestionsOnly
            ? `Mostrando apenas itens com sugestão (${roots.length})`
            : `Todo o histórico${summary ? ` — ${summary.count} itens` : ''}`}
          {items.length >= LIST_LIMIT
            ? ` (exibindo máx. ${LIST_LIMIT}; use os filtros para refinar)`
            : ''}
        </p>
      </div>

      <ItemFilters
        value={filters}
        onChange={setFilters}
        categories={categories}
        allTags={allTags}
        banks={banks}
        showSort
        searchPlaceholder="Buscar em todo o histórico…"
      />

      {aiSuggestions.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2 rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
          <span className="font-medium">✨ {doneSuggestions.length} sugestão(ões) da IA</span>
          <button
            onClick={() => {
              setReviewStartId(null)
              setReviewOpen(true)
            }}
            disabled={doneSuggestions.length === 0}
            className="rounded border border-amber-400/60 px-2 py-0.5 text-xs font-medium text-amber-200 hover:bg-amber-400/10 disabled:opacity-50"
            title="Revisar as sugestões uma a uma (categoria + tags)"
          >
            Revisar uma a uma →
          </button>
          <label className="flex items-center gap-1.5 text-xs text-amber-300/80" title="Mostrar apenas os itens com sugestão pendente">
            <input
              type="checkbox"
              checked={suggestionsOnly}
              onChange={(e) => setSuggestionsOnly(e.target.checked)}
              className="accent-amber-400"
            />
            Só itens com sugestão
          </label>
          <div className="ml-auto flex items-center gap-2">
            <button
              onClick={() => applyAllSug.mutate()}
              disabled={applyAllSug.isPending}
              className="rounded bg-amber-300 px-2.5 py-1 text-xs font-semibold text-amber-950 hover:bg-amber-200 disabled:opacity-50"
            >
              Aplicar todas
            </button>
            <button
              onClick={() => dismissAllSug.mutate()}
              disabled={dismissAllSug.isPending}
              className="rounded border border-amber-500/50 px-2.5 py-1 text-xs text-amber-300 hover:bg-amber-500/10 disabled:opacity-50"
            >
              Ignorar todas
            </button>
          </div>
        </div>
      )}

      {selected.size > 0 && (
        <div className="fixed bottom-[max(1rem,env(safe-area-inset-bottom))] left-1/2 z-10 flex w-max max-w-[calc(100vw-2rem)] -translate-x-1/2 flex-wrap items-center justify-center gap-x-3 gap-y-2 rounded-2xl border border-zinc-700 bg-zinc-900/95 px-4 py-2 text-sm shadow-2xl shadow-black/50 backdrop-blur sm:rounded-full sm:px-5 sm:pr-2">
          <span className="font-medium text-zinc-100">
            {selected.size} selecionado{selected.size > 1 ? 's' : ''}
          </span>
          <button
            onClick={() => setBulkOpen(true)}
            className="rounded-full bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-zinc-200"
          >
            Editar seleção
          </button>
          <button
            onClick={() => tagWithAi.mutate({ ids: [...selected], kind: 'tags' })}
            disabled={tagWithAi.isPending}
            title="Enfileira a IA para sugerir categoria e tags para os itens selecionados (revise um a um no modal)"
            className="rounded-full border border-amber-500/60 px-3 py-1.5 text-sm font-medium text-amber-300 hover:bg-amber-500/10 disabled:opacity-50"
          >
            {tagWithAi.isPending ? 'Enfileirando…' : '✨ Sugerir tags + categoria'}
          </button>
          <button
            onClick={() => setSelected(new Set())}
            className="rounded-full px-3 py-1.5 text-sm text-zinc-400 hover:text-zinc-100"
          >
            Limpar
          </button>
        </div>
      )}
      {reviewOpen && doneSuggestions.length > 0 && (
        <SuggestionReviewModal
          suggestions={doneSuggestions}
          startId={reviewStartId ?? undefined}
          categories={categories}
          onApply={(id, opts) => applySug.mutate({ id, ...opts })}
          onDismiss={(id) => dismissSug.mutate(id)}
          onClose={() => setReviewOpen(false)}
        />
      )}
      {aiWorking && !aiTagError && !failedBatch && (
        <div className="fixed bottom-20 left-1/2 z-10 -translate-x-1/2 rounded border border-cyan-500/40 bg-cyan-950/90 px-4 py-2 text-sm text-cyan-300 shadow-xl">
          IA processando {aiWorkCount} itens… as sugestões aparecem quando terminar
        </div>
      )}
      {(aiTagError || failedBatch) && (
        <div className="fixed bottom-20 left-1/2 z-10 -translate-x-1/2 max-w-[90vw] rounded border border-red-500/40 bg-red-950/90 px-4 py-2 text-sm text-red-300 shadow-xl">
          {aiTagError ||
            `Falha ao processar o lote de ${failedBatch?.kind === 'categorize' ? 'categorias' : 'tags'} (${failedBatch?.item_count} itens). ${failedBatch?.error_message ?? 'Tente novamente.'}`}
        </div>
      )}

      <div className="rounded border border-zinc-800 bg-zinc-900">
        <div className="flex items-center">
          <button
            onClick={() => setListOpen(!listOpen)}
            className="flex flex-1 items-center gap-2 px-4 py-3 text-sm font-medium"
          >
            Itens
            <span className="ml-auto text-zinc-500">{listOpen ? '▾' : '▸'}</span>
          </button>
          {roots.length > 0 && (
            <button
              onClick={() => setSelected(allSelected ? new Set() : new Set(rootIds))}
              className="px-4 py-3 text-xs text-zinc-400 hover:text-zinc-100"
            >
              {allSelected ? 'Limpar seleção' : 'Selecionar todos'}
            </button>
          )}
        </div>
        {listOpen && (
          <div className="border-t border-zinc-800">
            <div className="flex items-center justify-between px-4 py-2 text-sm">
              <span className="tabular-nums font-medium">Total: {totalLabel}</span>
              <span className="text-xs text-zinc-500">
                {summary ? `${summary.count} item${summary.count === 1 ? '' : 's'}` : ''}
              </span>
            </div>
            <div
              className={`px-2 py-2 transition-opacity ${isPlaceholderData ? 'opacity-60' : ''}`}
            >
              {isLoading ? (
                <p className="px-2 text-zinc-500">carregando…</p>
              ) : roots.length === 0 ? (
                <p className="px-2 text-zinc-500">Nenhum item encontrado.</p>
              ) : (
                <div className="divide-y divide-zinc-900">{roots.map(renderRoot)}</div>
              )}
            </div>
          </div>
        )}
      </div>

      {bulkOpen && (
        <BulkEditModal
          ids={[...selected]}
          onClose={() => setBulkOpen(false)}
          onApply={(input) => bulk.mutate(input)}
        />
      )}
      {form.open && (
        <ItemForm
          month={formMonth}
          editing={form.editing}
          onClose={() => {
            setForm({ open: false })
            qc.invalidateQueries({ queryKey: ['items'] })
            qc.invalidateQueries({ queryKey: ['dashboard'] })
          }}
        />
      )}
    </div>
  )
}
