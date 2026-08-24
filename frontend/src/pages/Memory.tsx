import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  categoriesApi,
  memoryApi,
  type MemoryEntry,
  type MemoryUpdateInput,
} from '../api/client'

interface RowProps {
  entry: MemoryEntry
  categories: { id: string; name: string }[]
  onUpdate: (vars: { id: string; input: MemoryUpdateInput }) => void
  onDelete: (id: string) => void
  onApplyAll: (merchant: string) => void
}

function MemoryRow({ entry, categories, onUpdate, onDelete, onApplyAll }: RowProps) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded border border-zinc-800 bg-zinc-900 px-3 py-2">
      <span className="w-48 truncate text-sm font-medium" title={entry.merchant}>
        {entry.merchant}
      </span>
      <select
        value={entry.category_id ?? ''}
        onChange={(e) =>
          onUpdate({ id: entry.id, input: { category_id: e.target.value || null } })
        }
        className="field w-44"
      >
        <option value="">Sem categoria</option>
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
      <span className="text-xs text-zinc-500" title="confirmações">
        ×{entry.confirm_count}
      </span>
      <button
        onClick={() => onApplyAll(entry.merchant)}
        className="ml-auto text-xs text-zinc-500 hover:text-zinc-200"
      >
        aplicar a todos
      </button>
      <button
        onClick={() => onDelete(entry.id)}
        className="text-xs text-zinc-500 hover:text-red-400"
      >
        apagar
      </button>
    </div>
  )
}

export default function Memory() {
  const qc = useQueryClient()
  const { data: entries = [] } = useQuery({ queryKey: ['memory'], queryFn: memoryApi.list })
  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })

  const [merchant, setMerchant] = useState('')
  const [categoryId, setCategoryId] = useState('')

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['memory'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const create = useMutation({
    mutationFn: memoryApi.create,
    onSuccess: () => {
      invalidate()
      setMerchant('')
      setCategoryId('')
    },
  })
  const update = useMutation({
    mutationFn: (vars: { id: string; input: MemoryUpdateInput }) => memoryApi.update(vars.id, vars.input),
    onSuccess: invalidate,
  })
  const remove = useMutation({ mutationFn: memoryApi.remove, onSuccess: invalidate })
  const applyAll = useMutation({ mutationFn: memoryApi.applyAll, onSuccess: invalidate })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!merchant.trim()) return
    create.mutate({
      merchant: merchant.trim(),
      category_id: categoryId || null,
    })
  }

  return (
    <div>
      <h1 className="mb-2 text-xl font-bold">Memória</h1>
      <p className="mb-4 text-sm text-zinc-500">
        Comerciante → categoria aprendida. Alimenta a IA e os botões “aplicar”.
      </p>

      <form onSubmit={submit} className="mb-6 flex flex-wrap gap-2">
        <input
          value={merchant}
          onChange={(e) => setMerchant(e.target.value)}
          placeholder="Comerciante"
          className="field w-48"
        />
        <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} className="field w-44">
          <option value="">Sem categoria</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <button className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900">
          Adicionar
        </button>
      </form>

      <div className="space-y-2">
        {entries.length === 0 ? (
          <p className="text-sm text-zinc-500">Nenhuma memória ainda.</p>
        ) : (
          entries.map((en) => (
            <MemoryRow
              key={en.id}
              entry={en}
              categories={categories}
              onUpdate={update.mutate}
              onDelete={remove.mutate}
              onApplyAll={applyAll.mutate}
            />
          ))
        )}
      </div>
    </div>
  )
}
