import { FormEvent, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  categoriesApi,
  tagsApi,
  type BulkItemUpdateInput,
} from '../api/client'

interface Props {
  ids: string[]
  onClose: () => void
  onApply: (input: BulkItemUpdateInput) => void
}

// '' = keep, '__clear' = set to null (no category).
const CLEAR_CATEGORY = '__clear'

const KIND_OPTIONS: [string, string][] = [
  ['expense', 'Despesa'],
  ['income', 'Receita'],
  ['refund', 'Estorno'],
  ['internal', 'Interna'],
]

export default function BulkEditModal({ ids, onClose, onApply }: Props) {
  const { data: categories = [] } = useQuery({
    queryKey: ['categories'],
    queryFn: categoriesApi.list,
  })
  const { data: allTags = [] } = useQuery({
    queryKey: ['tags'],
    queryFn: tagsApi.list,
  })

  const [kind, setKind] = useState('')
  const [categoryId, setCategoryId] = useState('')
  const [mode, setMode] = useState<'replace' | 'add' | 'remove'>('replace')
  const [tags, setTags] = useState<string[]>([])
  const [tagInput, setTagInput] = useState('')
  const [remember, setRemember] = useState(false)
  const [busy, setBusy] = useState(false)

  const addTag = () => {
    const t = tagInput.trim().replace(/,$/, '')
    if (t && !tags.includes(t)) {
      setTags([...tags, t])
    }
    setTagInput('')
  }

  const categoryChosen = categoryId !== '' && categoryId !== CLEAR_CATEGORY

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)

    const input: BulkItemUpdateInput = { ids }
    if (kind) input.kind = kind
    if (categoryId === CLEAR_CATEGORY) {
      input.category_id = null
    } else if (categoryId) {
      input.category_id = categoryId
    }
    // Empty tag list = "don't touch tags" (safety: never wipe tags by accident).
    if (tags.length > 0) {
      input.tags = tags
      input.tags_mode = mode
    }
    if (remember && categoryChosen) {
      input.update_memory = true
    }

    try {
      onApply(input)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-30 grid place-items-center bg-black/60 p-4"
      onClick={onClose}
    >
      <form
        onSubmit={submit}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-md space-y-3 rounded-lg border border-zinc-800 bg-zinc-900 p-5"
      >
        <h2 className="font-semibold">
          Editar {ids.length} {ids.length === 1 ? 'item' : 'itens'}
        </h2>

        <select value={kind} onChange={(e) => setKind(e.target.value)} className="field">
          <option value="">Tipo: não alterar</option>
          {KIND_OPTIONS.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>

        <select
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
          className="field"
        >
          <option value="">Categoria: não alterar</option>
          <option value={CLEAR_CATEGORY}>Sem categoria</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>

        <div className="space-y-2">
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value as 'replace' | 'add' | 'remove')}
            className="field"
          >
            <option value="replace">Substituir tags</option>
            <option value="add">Adicionar tags</option>
            <option value="remove">Remover tags</option>
          </select>
          <div className="field flex flex-wrap items-center gap-1.5">
            {tags.map((t) => (
              <span
                key={t}
                className="flex items-center gap-1 rounded bg-zinc-800 px-1.5 py-0.5 text-xs"
              >
                {t}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((x) => x !== t))}
                  className="text-zinc-500 hover:text-zinc-200"
                >
                  ×
                </button>
              </span>
            ))}
            <input
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ',') {
                  e.preventDefault()
                  addTag()
                }
              }}
              onBlur={addTag}
              placeholder={tags.length === 0 ? 'Tags (Enter para adicionar)' : ''}
              list="bulk-tags-list"
              className="min-w-[7rem] flex-1 bg-transparent text-sm outline-none"
            />
            <datalist id="bulk-tags-list">
              {allTags.map((t) => (
                <option key={t} value={t} />
              ))}
            </datalist>
          </div>
          <p className="text-[11px] text-zinc-500">
            Deixe as tags vazias para não alterar as tags atuais.
          </p>
        </div>

        <label className="flex items-center gap-2 text-xs text-zinc-400">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            disabled={!categoryChosen}
            className="checkbox"
          />
          Atualizar memória de categorização (por comerciante)
        </label>

        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-zinc-700 px-3 py-1.5 text-sm"
          >
            Cancelar
          </button>
          <button
            disabled={busy}
            className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-50"
          >
            Aplicar
          </button>
        </div>
      </form>
    </div>
  )
}
