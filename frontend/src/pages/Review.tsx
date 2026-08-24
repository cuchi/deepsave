import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { categoriesApi, itemsApi, matchesApi, memoryApi } from '../api/client'
import type { Item } from '../lib/types'
import { fmtCents } from '../lib/format'
import ItemForm from '../components/ItemForm'

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

export default function Review() {
  const qc = useQueryClient()
  const [editing, setEditing] = useState<Item | null>(null)
  const [itemsOpen, setItemsOpen] = useState(true)
  const [linksOpen, setLinksOpen] = useState(true)

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

  const memoryByMerchant = new Map(memory.map((m) => [m.merchant, m]))
  const catName = (id: string | null) => categories.find((c) => c.id === id)?.name

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['items', 'pending'] })
    qc.invalidateQueries({ queryKey: ['matches'] })
    qc.invalidateQueries({ queryKey: ['documents'] })
    qc.invalidateQueries({ queryKey: ['memory'] })
    qc.invalidateQueries({ queryKey: ['categories'] })
  }

  const confirmItem = useMutation({ mutationFn: itemsApi.confirm, onSuccess: invalidate })
  const rejectItem = useMutation({ mutationFn: itemsApi.reject, onSuccess: invalidate })
  const acceptSuggestion = useMutation({ mutationFn: itemsApi.acceptSuggestion, onSuccess: invalidate })
  const applyMemory = useMutation({ mutationFn: itemsApi.applyMemory, onSuccess: invalidate })
  const applyAllMemory = useMutation({ mutationFn: memoryApi.applyAll, onSuccess: invalidate })
  const acceptMatch = useMutation({ mutationFn: matchesApi.accept, onSuccess: invalidate })
  const rejectMatch = useMutation({ mutationFn: matchesApi.reject, onSuccess: invalidate })
  const suggest = useMutation({ mutationFn: matchesApi.suggest, onSuccess: invalidate })

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
