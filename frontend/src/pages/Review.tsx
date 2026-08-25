import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { aiTagsApi, categoriesApi, itemsApi, matchesApi, memoryApi, tagsApi } from '../api/client'
import type { Item, SuggestionDetail } from '../lib/types'
import { fmtCents } from '../lib/format'
import ItemForm from '../components/ItemForm'

const BATCH_STATUS: Record<string, [string, string]> = {
  pending: ['Aguardando', 'text-amber-300 bg-amber-500/10'],
  processing: ['Processando…', 'text-cyan-300 bg-cyan-500/10'],
  done: ['Concluído', 'text-emerald-300 bg-emerald-500/10'],
  failed: ['Falhou', 'text-red-300 bg-red-500/10'],
}

const KIND_LABELS: Record<string, string> = {
  expense: 'Despesa',
  income: 'Receita',
  refund: 'Estorno',
  card_payment: 'Pagamento de fatura',
  investment: 'Investimento',
  internal: 'Interna',
}

function normMerchant(s: string): string {
  return s
    .trim()
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
}

/** One AI tag proposal with inline editing: remove chips, add new ones, apply. */
function SuggestionRow({
  s,
  onApply,
  onDismiss,
  busy,
}: {
  s: SuggestionDetail
  onApply: (id: string, tags: string[]) => void
  onDismiss: (id: string) => void
  busy: boolean
}) {
  const [tags, setTags] = useState<string[]>(s.suggested_tags)
  const [input, setInput] = useState('')

  const addTag = () => {
    const t = input.trim().replace(/,$/, '')
    if (t && !tags.includes(t)) {
      setTags([...tags, t])
    }
    setInput('')
  }

  return (
    <div className="rounded border border-zinc-800 bg-zinc-950 px-3 py-2">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{s.description}</p>
          <p className="truncate text-xs text-zinc-500">
            {s.occurred_on}
            {s.merchant ? ` · ${s.merchant}` : ''}
            {s.category_name ? ` · ${s.category_name}` : ''}
            {s.tags.length > 0 ? ` · atual: ${s.tags.join(', ')}` : ''}
          </p>
        </div>
        <span className="shrink-0 text-sm tabular-nums">{fmtCents(s.amount_cents)}</span>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        {tags.map((t) => (
          <span
            key={t}
            className="flex items-center gap-1 rounded bg-amber-500/15 px-1.5 py-0.5 text-xs text-amber-300"
          >
            {t}
            <button
              type="button"
              onClick={() => setTags(tags.filter((x) => x !== t))}
              className="text-amber-400/70 hover:text-amber-100"
            >
              ×
            </button>
          </span>
        ))}
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault()
              addTag()
            }
          }}
          onBlur={addTag}
          placeholder={tags.length === 0 ? 'sem tags sugeridas — adicione e aplique…' : ''}
          list="ai-tags-list"
          className="min-w-[8rem] flex-1 bg-transparent text-xs outline-none placeholder:text-zinc-600"
        />
        <span className="ml-auto flex shrink-0 items-center gap-2">
          <button
            onClick={() => onDismiss(s.id)}
            disabled={busy}
            className="text-xs text-zinc-500 hover:text-red-400 disabled:opacity-50"
          >
            ignorar
          </button>
          <button
            onClick={() => onApply(s.id, tags)}
            disabled={busy}
            className="rounded bg-amber-500/20 px-2 py-1 text-xs font-medium text-amber-300 hover:bg-amber-500/30 disabled:opacity-50"
          >
            aplicar
          </button>
        </span>
      </div>
    </div>
  )
}

export default function Review() {
  const qc = useQueryClient()
  const [editing, setEditing] = useState<Item | null>(null)
  const [itemsOpen, setItemsOpen] = useState(true)
  const [linksOpen, setLinksOpen] = useState(true)
  const [tagsOpen, setTagsOpen] = useState(true)

  const { data: items = [] } = useQuery({
    queryKey: ['items', 'pending'],
    queryFn: () => itemsApi.listByStatus('pending_review'),
    refetchInterval: 5000,
  })
  const { data: categories = [] } = useQuery({
    queryKey: ['categories'],
    queryFn: categoriesApi.list,
  })
  const { data: matches = [] } = useQuery({
    queryKey: ['matches'],
    queryFn: () => matchesApi.list('suggested'),
  })
  const { data: memory = [] } = useQuery({
    queryKey: ['memory'],
    queryFn: memoryApi.list,
  })
  const { data: tagBatches = [] } = useQuery({
    queryKey: ['ai-tag-batches'],
    queryFn: aiTagsApi.listBatches,
    refetchInterval: 5000,
  })
  // Only pending (not yet reviewed) suggestions — they disappear on apply/dismiss.
  const { data: suggestions = [] } = useQuery({
    queryKey: ['ai-tag-suggestions'],
    queryFn: () => aiTagsApi.listSuggestions(),
    refetchInterval: 5000,
  })
  const { data: allTags = [] } = useQuery({
    queryKey: ['tags'],
    queryFn: tagsApi.list,
  })

  const memoryByMerchant = new Map(memory.map((m) => [m.merchant, m]))
  const catName = (id: string | null) => categories.find((c) => c.id === id)?.name
  const suggestionsByBatch = useMemo(() => {
    const m = new Map<string, SuggestionDetail[]>()
    for (const s of suggestions) {
      const arr = m.get(s.batch_id) ?? []
      arr.push(s)
      m.set(s.batch_id, arr)
    }
    return m
  }, [suggestions])

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['items', 'pending'] })
    qc.invalidateQueries({ queryKey: ['items'] })
    qc.invalidateQueries({ queryKey: ['matches'] })
    qc.invalidateQueries({ queryKey: ['documents'] })
    qc.invalidateQueries({ queryKey: ['memory'] })
    qc.invalidateQueries({ queryKey: ['categories'] })
    qc.invalidateQueries({ queryKey: ['tags'] })
    qc.invalidateQueries({ queryKey: ['ai-tag-batches'] })
    qc.invalidateQueries({ queryKey: ['ai-tag-suggestions'] })
  }

  const confirmItem = useMutation({ mutationFn: itemsApi.confirm, onSuccess: invalidate })
  const rejectItem = useMutation({ mutationFn: itemsApi.reject, onSuccess: invalidate })
  const acceptSuggestion = useMutation({ mutationFn: itemsApi.acceptSuggestion, onSuccess: invalidate })
  const applyMemory = useMutation({ mutationFn: itemsApi.applyMemory, onSuccess: invalidate })
  const applyAllMemory = useMutation({ mutationFn: memoryApi.applyAll, onSuccess: invalidate })
  const acceptMatch = useMutation({ mutationFn: matchesApi.accept, onSuccess: invalidate })
  const rejectMatch = useMutation({ mutationFn: matchesApi.reject, onSuccess: invalidate })
  const suggest = useMutation({ mutationFn: matchesApi.suggest, onSuccess: invalidate })
  const applySuggestion = useMutation({
    mutationFn: ({ id, tags }: { id: string; tags?: string[] }) => aiTagsApi.apply(id, tags),
    onSuccess: invalidate,
  })
  const dismissSuggestion = useMutation({ mutationFn: aiTagsApi.dismiss, onSuccess: invalidate })
  const applyAllSuggestions = useMutation({ mutationFn: aiTagsApi.applyAll, onSuccess: invalidate })
  const dismissAllSuggestions = useMutation({ mutationFn: aiTagsApi.dismissAll, onSuccess: invalidate })

  return (
    <div>
      <h1 className="mb-4 text-xl font-bold">Revisar</h1>

      {/* Pending items */}
      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setItemsOpen(!itemsOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Itens para confirmar
          {items.length > 0 && (
            <span className="rounded bg-amber-500/20 px-1.5 py-0.5 text-xs text-amber-300">
              {items.length}
            </span>
          )}
          <span className="ml-auto text-zinc-500">{itemsOpen ? '▾' : '▸'}</span>
        </button>
        {itemsOpen && (
          <div className="border-t border-zinc-800 p-3">
            {items.length === 0 ? (
              <p className="text-sm text-zinc-500">Nada pendente. 🎉</p>
            ) : (
              <div className="space-y-2">
                {items.map((it) => {
                  const mem = it.merchant ? memoryByMerchant.get(normMerchant(it.merchant)) : undefined
                  return (
                  <div
                    key={it.id}
                    className="flex items-center gap-3 rounded border border-zinc-800 bg-zinc-950 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium">{it.description}</p>
                      <p className="truncate text-xs text-zinc-500">
                        {it.occurred_on} · {KIND_LABELS[it.kind] ?? it.kind}
                        {it.merchant ? ` · ${it.merchant}` : ''}
                        {catName(it.category_id) ? ` · ${catName(it.category_id)}` : ''}
                        {it.tags.length > 0 ? ` · ${it.tags.join(', ')}` : ''}
                      </p>
                      {(it.suggested_category || mem) && (
                        <div className="mt-1 flex flex-wrap items-center gap-2">
                          {it.suggested_category && (
                            <span className="flex items-center gap-1 rounded bg-amber-500/15 px-1.5 py-0.5 text-[11px] text-amber-300">
                              Nova categoria: {it.suggested_category}
                              <button
                                onClick={() => acceptSuggestion.mutate(it.id)}
                                className="rounded bg-amber-500/30 px-1 text-[10px] text-amber-100 hover:bg-amber-500/50"
                              >
                                criar
                              </button>
                            </span>
                          )}
                          {mem && (
                            <span className="flex items-center gap-1 rounded bg-zinc-800 px-1.5 py-0.5 text-[11px] text-zinc-400">
                              Usual: {mem.category_name}
                              <button
                                onClick={() => applyMemory.mutate(it.id)}
                                className="rounded bg-zinc-700 px-1 text-[10px] text-zinc-100 hover:bg-zinc-600"
                              >
                                aplicar
                              </button>
                              <button
                                onClick={() => it.merchant && applyAllMemory.mutate(it.merchant)}
                                title="Aplicar a categoria a todos os itens deste comerciante"
                                className="rounded bg-zinc-700 px-1 text-[10px] text-zinc-100 hover:bg-zinc-600"
                              >
                                todos
                              </button>
                            </span>
                          )}
                        </div>
                      )}
                    </div>
                    <span className="ml-auto whitespace-nowrap text-sm tabular-nums">
                      {it.kind === 'income'
                        ? '+'
                        : it.kind === 'card_payment' || it.kind === 'investment' || it.kind === 'internal'
                          ? ''
                          : '−'}
                      {fmtCents(it.amount_cents)}
                    </span>
                    <button
                      onClick={() => setEditing(it)}
                      className="text-xs text-zinc-500 hover:text-zinc-200"
                    >
                      editar
                    </button>
                    <button
                      onClick={() => rejectItem.mutate(it.id)}
                      className="text-xs text-zinc-500 hover:text-red-400"
                    >
                      rejeitar
                    </button>
                    <button
                      onClick={() => confirmItem.mutate(it.id)}
                      className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                    >
                      confirmar
                    </button>
                  </div>
                  )
                })}
              </div>
            )}
          </div>
        )}
      </div>

      {/* AI tag suggestions */}
      <div className="mb-4 rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setTagsOpen(!tagsOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Tags sugeridas pela IA
          {suggestions.length > 0 && (
            <span className="rounded bg-amber-500/20 px-1.5 py-0.5 text-xs text-amber-300">
              {suggestions.length}
            </span>
          )}
          {tagBatches.some((b) => b.status === 'pending' || b.status === 'processing') && (
            <span className="rounded bg-cyan-500/20 px-1.5 py-0.5 text-xs text-cyan-300">
              processando
            </span>
          )}
          <span className="ml-auto text-zinc-500">{tagsOpen ? '▾' : '▸'}</span>
        </button>
        {tagsOpen && (
          <div className="border-t border-zinc-800 p-3">
            {tagBatches.length === 0 ? (
              <p className="text-sm text-zinc-500">
                Nenhum lote ainda. Selecione itens no mês e use “Taggear com IA”.
              </p>
            ) : (
              <div className="space-y-3">
                {tagBatches.map((batch) => {
                  const [label, chipCls] =
                    BATCH_STATUS[batch.status] ?? [batch.status, 'text-zinc-400 bg-zinc-800']
                  const batchSuggestions = suggestionsByBatch.get(batch.id) ?? []
                  return (
                    <div
                      key={batch.id}
                      className="rounded border border-zinc-800 bg-zinc-950 p-3"
                    >
                      <div className="flex flex-wrap items-center gap-2 text-sm">
                        <span className="text-zinc-300">
                          Lote de {new Date(batch.created_at).toLocaleString('pt-BR')}
                        </span>
                        <span className={`rounded px-1.5 py-0.5 text-xs ${chipCls}`}>{label}</span>
                        <span className="text-xs text-zinc-500">{batch.item_count} itens</span>
                        {batch.status === 'done' && batchSuggestions.length > 0 && (
                          <span className="ml-auto flex shrink-0 items-center gap-2">
                            <button
                              onClick={() => dismissAllSuggestions.mutate(batch.id)}
                              disabled={dismissAllSuggestions.isPending}
                              className="rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-400 hover:text-red-400 disabled:opacity-50"
                            >
                              ignorar tudo
                            </button>
                            <button
                              onClick={() => applyAllSuggestions.mutate(batch.id)}
                              disabled={applyAllSuggestions.isPending}
                              className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900 disabled:opacity-50"
                            >
                              aplicar tudo
                            </button>
                          </span>
                        )}
                        {batch.status === 'failed' && batch.error_message && (
                          <span className="w-full text-xs text-red-400">{batch.error_message}</span>
                        )}
                      </div>
                      {batch.status === 'done' && batchSuggestions.length > 0 && (
                        <div className="mt-2 space-y-2">
                          {batchSuggestions.map((s) => (
                            <SuggestionRow
                              key={s.id}
                              s={s}
                              onApply={(id, tags) => applySuggestion.mutate({ id, tags })}
                              onDismiss={(id) => dismissSuggestion.mutate(id)}
                              busy={applySuggestion.isPending || dismissSuggestion.isPending}
                            />
                          ))}
                        </div>
                      )}
                      {batch.status === 'done' && batchSuggestions.length === 0 && (
                        <p className="mt-2 text-xs text-zinc-500">
                          Todas as sugestões deste lote já foram revisadas.
                        </p>
                      )}
                    </div>
                  )
                })}
              </div>
            )}
            <datalist id="ai-tags-list">
              {allTags.map((t) => (
                <option key={t} value={t} />
              ))}
            </datalist>
          </div>
        )}
      </div>

      {/* Suggested links */}
      <div className="rounded border border-zinc-800 bg-zinc-900">
        <button
          onClick={() => setLinksOpen(!linksOpen)}
          className="flex w-full items-center gap-2 px-4 py-3 text-sm font-medium"
        >
          Vínculos sugeridos
          {matches.length > 0 && (
            <span className="rounded bg-amber-500/20 px-1.5 py-0.5 text-xs text-amber-300">
              {matches.length}
            </span>
          )}
          <span className="ml-auto text-zinc-500">{linksOpen ? '▾' : '▸'}</span>
        </button>
        {linksOpen && (
          <div className="border-t border-zinc-800 p-3">
            <div className="mb-2 flex items-center">
              <p className="text-xs text-zinc-500">
                Recibo → lançamento de cartão/conta.
              </p>
              <button
                onClick={() => suggest.mutate()}
                className="ml-auto rounded bg-zinc-100 px-3 py-1.5 text-xs font-medium text-zinc-900"
              >
                Sugerir vínculos
              </button>
            </div>

            {matches.length === 0 ? (
              <p className="text-sm text-zinc-500">Nenhuma sugestão de vínculo.</p>
            ) : (
              <div className="space-y-2">
                {matches.map((m) => (
                  <div
                    key={m.id}
                    className="rounded border border-zinc-800 bg-zinc-950 px-3 py-2 text-sm"
                  >
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{m.child.description}</span>
                      <span className="text-zinc-500">→</span>
                      <span className="font-medium">{m.parent.description}</span>
                      <span className="ml-auto text-xs text-zinc-500">
                        {(m.confidence * 100).toFixed(0)}%
                      </span>
                      <button
                        onClick={() => rejectMatch.mutate(m.id)}
                        className="text-xs text-zinc-500 hover:text-red-400"
                      >
                        ignorar
                      </button>
                      <button
                        onClick={() => acceptMatch.mutate(m.id)}
                        className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                      >
                        vincular
                      </button>
                    </div>
                    <div className="mt-1 text-xs text-zinc-500">
                      {fmtCents(m.child.amount_cents)} dentro de {fmtCents(m.parent.amount_cents)} ·{' '}
                      {m.child.occurred_on} vs {m.parent.occurred_on}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {editing && (
        <ItemForm
          month={editing.occurred_on.slice(0, 7)}
          editing={editing}
          onClose={() => {
            setEditing(null)
            invalidate()
          }}
        />
      )}
    </div>
  )
}
