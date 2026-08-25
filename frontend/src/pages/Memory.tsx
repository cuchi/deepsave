import { FormEvent, useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useSearchParams } from 'react-router-dom'
import {
  categoriesApi,
  memoryApi,
  tagsApi,
  type MemoryEntry,
  type MemoryPreviewItem,
  type MemoryUpdateInput,
} from '../api/client'
import { fmtCents } from '../lib/format'

type Section = 'memory' | 'categories' | 'tags'

// ---------------------------------------------------------------------------
// Memory rows (merchant → category + tags)
// ---------------------------------------------------------------------------

interface RowProps {
  entry: MemoryEntry
  categories: { id: string; name: string }[]
  onUpdate: (vars: { id: string; input: MemoryUpdateInput }) => void
  onDelete: (id: string) => void
  onApply: (merchant: string) => void
}

function MemoryRow({ entry, categories, onUpdate, onDelete, onApply }: RowProps) {
  const [categoryId, setCategoryId] = useState(entry.category_id ?? '')
  const [tags, setTags] = useState<string[]>(entry.tags)
  const [tagInput, setTagInput] = useState('')

  const commit = (nextCat: string, nextTags: string[]) => {
    onUpdate({ id: entry.id, input: { category_id: nextCat || null, tags: nextTags } })
  }

  const addTag = () => {
    const t = tagInput.trim().toLowerCase()
    if (t && !tags.includes(t)) {
      const next = [...tags, t]
      setTags(next)
      commit(categoryId, next)
    }
    setTagInput('')
  }

  const removeTag = (t: string) => {
    const next = tags.filter((x) => x !== t)
    setTags(next)
    commit(categoryId, next)
  }

  return (
    <div className="flex flex-wrap items-center gap-2 rounded border border-zinc-800 bg-zinc-900 px-3 py-2">
      <span className="w-44 truncate text-sm font-medium" title={entry.merchant}>
        {entry.merchant}
      </span>
      <select
        value={categoryId}
        onChange={(e) => {
          setCategoryId(e.target.value)
          commit(e.target.value, tags)
        }}
        className="field w-40"
      >
        <option value="">Sem categoria</option>
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>

      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
        {tags.map((t) => (
          <span
            key={t}
            className="flex items-center gap-1 rounded bg-zinc-800 px-1.5 py-0.5 text-xs text-zinc-300"
          >
            {t}
            <button
              type="button"
              onClick={() => removeTag(t)}
              className="text-zinc-500 hover:text-red-400"
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
          placeholder="+ tag"
          className="min-w-[5rem] flex-1 bg-transparent text-xs outline-none placeholder:text-zinc-600"
        />
      </div>

      <span className="text-xs text-zinc-500" title="confirmações">
        ×{entry.confirm_count}
      </span>
      <button
        onClick={() => onApply(entry.merchant)}
        className="ml-auto text-xs text-zinc-400 hover:text-zinc-100"
        title="Ver os itens que mudariam e escolher quais aplicar"
      >
        aplicar
      </button>
      <button onClick={() => onDelete(entry.id)} className="text-xs text-zinc-500 hover:text-red-400">
        apagar
      </button>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Preview-before-apply panel
// ---------------------------------------------------------------------------

function PreviewPanel({
  merchant,
  items,
  loading,
  onClose,
  onApply,
  busy,
}: {
  merchant: string | null
  items: MemoryPreviewItem[]
  loading: boolean
  onClose: () => void
  onApply: (ids: string[]) => void
  busy: boolean
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set())

  // Default: everything selected — the user reviews and unchecks what to skip.
  useEffect(() => {
    setSelected(new Set(items.map((i) => i.item_id)))
  }, [items])

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }
  const toggleAll = () => {
    setSelected((prev) =>
      prev.size === items.length ? new Set() : new Set(items.map((i) => i.item_id)),
    )
  }

  const title = merchant ? `“${merchant}”` : 'todas as memórias'

  return (
    <div className="mb-6 rounded border border-amber-500/30 bg-zinc-900">
      <div className="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
        <h2 className="text-sm font-bold">Prévia — aplicar memória de {title}</h2>
        <span className="text-xs text-zinc-500">
          {loading ? 'carregando…' : `${items.length} item(ns) mudariam`}
        </span>
        <button onClick={onClose} className="ml-auto text-xs text-zinc-500 hover:text-zinc-200">
          fechar ✕
        </button>
      </div>

      {loading ? (
        <p className="px-4 py-6 text-sm text-zinc-500">Calculando itens afetados…</p>
      ) : items.length === 0 ? (
        <p className="px-4 py-6 text-sm text-zinc-500">
          Nada a aplicar — nenhum item mudaria com esta memória. 🎉
        </p>
      ) : (
        <>
          <div className="max-h-80 overflow-y-auto">
            <div className="flex items-center gap-2 border-b border-zinc-800 px-4 py-2 text-xs font-medium uppercase tracking-wide text-zinc-500">
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={selected.size === items.length} onChange={toggleAll} />
                <span>Todos</span>
              </label>
              <span className="ml-auto">Valor</span>
            </div>
            <ul className="divide-y divide-zinc-800">
              {items.map((it) => {
                const catChange = it.changes.includes('category')
                const tagChange = it.changes.includes('tags')
                return (
                  <li key={it.item_id} className="flex items-center gap-2 px-4 py-2">
                    <input
                      type="checkbox"
                      checked={selected.has(it.item_id)}
                      onChange={() => toggle(it.item_id)}
                    />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm">{it.description}</p>
                      <p className="truncate text-xs text-zinc-500">
                        {it.occurred_on} · {it.merchant}
                      </p>
                      <p className="mt-0.5 flex flex-wrap items-center gap-1 text-xs">
                        {catChange && (
                          <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-amber-300">
                            categoria: {it.current_category ?? '—'} → {it.proposed_category ?? '—'}
                          </span>
                        )}
                        {tagChange && (
                          <span className="rounded bg-cyan-500/15 px-1.5 py-0.5 text-cyan-300">
                            + tags: {it.tags_to_add.join(', ')}
                          </span>
                        )}
                      </p>
                    </div>
                    <span className="shrink-0 text-sm tabular-nums">{fmtCents(it.amount_cents)}</span>
                  </li>
                )
              })}
            </ul>
          </div>
          <div className="flex items-center justify-end gap-3 border-t border-zinc-800 px-4 py-3">
            <span className="text-xs text-zinc-500">
              {selected.size} selecionado(s)
            </span>
            <button
              onClick={() => onApply([...selected])}
              disabled={selected.size === 0 || busy}
              className="rounded bg-amber-500 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-40"
            >
              {busy ? 'Aplicando…' : `Aplicar selecionados (${selected.size})`}
            </button>
          </div>
        </>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Categories section (folded-in)
// ---------------------------------------------------------------------------

function CategoriesSection() {
  const qc = useQueryClient()
  const { data: cats = [] } = useQuery({
    queryKey: ['categories'],
    queryFn: categoriesApi.list,
  })

  const [name, setName] = useState('')
  const [color, setColor] = useState('')

  const create = useMutation({
    mutationFn: categoriesApi.create,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['categories'] })
      setName('')
      setColor('')
    },
  })
  const remove = useMutation({
    mutationFn: categoriesApi.remove,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['categories'] }),
  })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    create.mutate({
      name: name.trim(),
      parent_id: null,
      color: color || null,
      icon: null,
    })
  }

  return (
    <div>
      <form onSubmit={submit} className="mb-6 flex gap-2">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Nome"
          className="field"
        />
        <input
          value={color}
          onChange={(e) => setColor(e.target.value)}
          placeholder="#cor"
          className="field w-28"
        />
        <button className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900">
          Adicionar
        </button>
      </form>

      <ul className="space-y-2">
        {cats.map((c) => (
          <li
            key={c.id}
            className="flex items-center gap-3 rounded border border-zinc-800 bg-zinc-900 px-3 py-2"
          >
            {c.color && <span className="h-3 w-3 rounded-full" style={{ background: c.color }} />}
            <span className="text-sm">{c.name}</span>
            <button
              onClick={() => remove.mutate(c.id)}
              className="ml-auto text-xs text-zinc-500 hover:text-red-400"
            >
              apagar
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Tags section (folded-in)
// ---------------------------------------------------------------------------

function TagsSection() {
  const qc = useQueryClient()
  const { data: usage = [] } = useQuery({
    queryKey: ['tags-usage'],
    queryFn: tagsApi.usage,
  })

  const [renaming, setRenaming] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [merging, setMerging] = useState<string | null>(null)
  const [mergeInto, setMergeInto] = useState('')

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['tags-usage'] })
    qc.invalidateQueries({ queryKey: ['tags'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const rename = useMutation({
    mutationFn: (v: { from: string; to: string }) => tagsApi.rename(v.from, v.to),
    onSuccess: invalidate,
  })
  const merge = useMutation({
    mutationFn: (v: { from: string; into: string }) => tagsApi.merge(v.from, v.into),
    onSuccess: invalidate,
  })
  const remove = useMutation({ mutationFn: tagsApi.remove, onSuccess: invalidate })

  const startRename = (tag: string) => {
    setRenaming(tag)
    setRenameValue(tag)
    setMerging(null)
  }
  const confirmRename = (tag: string) => {
    const next = renameValue.trim()
    if (next && next !== tag) rename.mutate({ from: tag, to: next })
    setRenaming(null)
    setRenameValue('')
  }
  const startMerge = (tag: string) => {
    setMerging(tag)
    setMergeInto('')
    setRenaming(null)
  }
  const confirmMerge = (tag: string) => {
    if (mergeInto && mergeInto !== tag) merge.mutate({ from: tag, into: mergeInto })
    setMerging(null)
    setMergeInto('')
  }
  const doDelete = (tag: string) => {
    if (window.confirm(`Apagar a tag "${tag}" de todos os itens?`)) {
      remove.mutate(tag)
    }
  }

  return (
    <div>
      <p className="mb-4 text-sm text-zinc-500">
        Renomear, mesclar ou apagar tags — as alterações valem para todos os itens (e memórias).
      </p>

      {usage.length === 0 ? (
        <p className="text-sm text-zinc-500">Nenhuma tag ainda.</p>
      ) : (
        <div className="overflow-hidden rounded border border-zinc-800">
          <div className="flex items-center gap-3 bg-zinc-900 px-3 py-2 text-xs font-medium uppercase tracking-wide text-zinc-500">
            <span className="flex-1">Tag</span>
            <span className="w-16 text-right">Itens</span>
            <span className="w-60 text-right">Ações</span>
          </div>
          <ul className="divide-y divide-zinc-800">
            {usage.map((u) => (
              <li key={u.tag} className="flex flex-wrap items-center gap-3 bg-zinc-950 px-3 py-2">
                <span className="min-w-0 flex-1 truncate font-mono text-sm">{u.tag}</span>
                <span className="w-16 text-right text-sm tabular-nums text-zinc-400">{u.count}</span>

                {renaming === u.tag ? (
                  <div className="flex w-60 items-center justify-end gap-2">
                    <input
                      autoFocus
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') confirmRename(u.tag)
                        if (e.key === 'Escape') setRenaming(null)
                      }}
                      className="field w-32 py-1 text-xs"
                    />
                    <button
                      onClick={() => confirmRename(u.tag)}
                      className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                    >
                      salvar
                    </button>
                    <button onClick={() => setRenaming(null)} className="text-xs text-zinc-500 hover:text-zinc-200">
                      cancelar
                    </button>
                  </div>
                ) : merging === u.tag ? (
                  <div className="flex w-60 items-center justify-end gap-2">
                    <select
                      autoFocus
                      value={mergeInto}
                      onChange={(e) => setMergeInto(e.target.value)}
                      className="field w-32 py-1 text-xs"
                    >
                      <option value="">Destino…</option>
                      {usage
                        .filter((o) => o.tag !== u.tag)
                        .map((o) => (
                          <option key={o.tag} value={o.tag}>
                            {o.tag}
                          </option>
                        ))}
                    </select>
                    <button
                      onClick={() => confirmMerge(u.tag)}
                      className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                    >
                      mesclar
                    </button>
                    <button onClick={() => setMerging(null)} className="text-xs text-zinc-500 hover:text-zinc-200">
                      cancelar
                    </button>
                  </div>
                ) : (
                  <div className="flex w-60 items-center justify-end gap-3 text-xs">
                    <button onClick={() => startRename(u.tag)} className="text-zinc-400 hover:text-zinc-100">
                      renomear
                    </button>
                    <button onClick={() => startMerge(u.tag)} className="text-zinc-400 hover:text-zinc-100">
                      mesclar
                    </button>
                    <button onClick={() => doDelete(u.tag)} className="text-zinc-500 hover:text-red-400">
                      apagar
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Merged page: Memória (memory + categories + tags)
// ---------------------------------------------------------------------------

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'memory', label: 'Memória' },
  { id: 'categories', label: 'Categorias' },
  { id: 'tags', label: 'Tags' },
]

export default function Memory() {
  const qc = useQueryClient()
  const [searchParams, setSearchParams] = useSearchParams()
  const [section, setSection] = useState<Section>('memory')

  const { data: entries = [] } = useQuery({ queryKey: ['memory'], queryFn: memoryApi.list })
  const { data: categories = [] } = useQuery({ queryKey: ['categories'], queryFn: categoriesApi.list })

  const [merchant, setMerchant] = useState('')
  const [categoryId, setCategoryId] = useState('')
  const [tagInput, setTagInput] = useState('')
  const [entryTags, setEntryTags] = useState<string[]>([])

  // Preview state: `merchant = null` means all memories.
  const [previewMerchant, setPreviewMerchant] = useState<string | null | undefined>(undefined)
  const [previewItems, setPreviewItems] = useState<MemoryPreviewItem[]>([])
  const [previewLoading, setPreviewLoading] = useState(false)
  const [applying, setApplying] = useState(false)

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['memory'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const openPreview = async (m: string | null) => {
    setPreviewMerchant(m)
    setPreviewLoading(true)
    setPreviewItems([])
    try {
      setPreviewItems(await memoryApi.preview(m))
    } finally {
      setPreviewLoading(false)
    }
  }

  // `?apply=<merchant>` opens the preview for that merchant (from the Review page).
  useEffect(() => {
    const apply = searchParams.get('apply')
    if (apply) {
      openPreview(apply)
      setSearchParams({}, { replace: true })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const create = useMutation({
    mutationFn: memoryApi.create,
    onSuccess: () => {
      invalidate()
      setMerchant('')
      setCategoryId('')
      setEntryTags([])
      setTagInput('')
    },
  })
  const update = useMutation({
    mutationFn: (vars: { id: string; input: MemoryUpdateInput }) => memoryApi.update(vars.id, vars.input),
    onSuccess: invalidate,
  })
  const remove = useMutation({ mutationFn: memoryApi.remove, onSuccess: invalidate })
  const applyMemory = useMutation({
    mutationFn: (ids: string[]) => memoryApi.apply(previewMerchant ?? null, ids),
    onSuccess: () => {
      invalidate()
      setPreviewMerchant(undefined)
    },
  })

  const addEntryTag = () => {
    const t = tagInput.trim().toLowerCase()
    if (t && !entryTags.includes(t)) setEntryTags([...entryTags, t])
    setTagInput('')
  }

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!merchant.trim()) return
    create.mutate({
      merchant: merchant.trim(),
      category_id: categoryId || null,
      tags: entryTags,
    })
  }

  const previewOpen = previewMerchant !== undefined

  return (
    <div>
      <div className="mb-4 flex items-center gap-3">
        <h1 className="text-xl font-bold">Memória</h1>
        {section === 'memory' && (
          <button
            onClick={() => openPreview(null)}
            className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900"
          >
            Aplicar todas
          </button>
        )}
      </div>

      <div className="mb-5 flex gap-1 border-b border-zinc-800">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            onClick={() => setSection(s.id)}
            className={`border-b-2 px-3 py-2 text-sm font-medium ${
              section === s.id
                ? 'border-zinc-100 text-zinc-100'
                : 'border-transparent text-zinc-500 hover:text-zinc-300'
            }`}
          >
            {s.label}
          </button>
        ))}
      </div>

      {previewOpen && (
        <PreviewPanel
          merchant={previewMerchant}
          items={previewItems}
          loading={previewLoading}
          onClose={() => setPreviewMerchant(undefined)}
          onApply={(ids) => {
            setApplying(true)
            applyMemory.mutate(ids, { onSettled: () => setApplying(false) })
          }}
          busy={applying}
        />
      )}

      {section === 'memory' && (
        <div>
          <p className="mb-4 text-sm text-zinc-500">
            Comerciante → categoria + tags aprendidos. Alimenta a IA e o botão “Aplicar”.
          </p>

          <form onSubmit={submit} className="mb-6 flex flex-wrap gap-2">
            <input
              value={merchant}
              onChange={(e) => setMerchant(e.target.value)}
              placeholder="Comerciante"
              className="field w-48"
            />
            <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} className="field w-40">
              <option value="">Sem categoria</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
            <div className="flex items-center gap-1 rounded border border-zinc-800 bg-zinc-900 px-2">
              {entryTags.map((t) => (
                <span key={t} className="flex items-center gap-1 rounded bg-zinc-800 px-1.5 py-0.5 text-xs">
                  {t}
                  <button
                    type="button"
                    onClick={() => setEntryTags(entryTags.filter((x) => x !== t))}
                    className="text-zinc-500 hover:text-red-400"
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
                    addEntryTag()
                  }
                }}
                onBlur={addEntryTag}
                placeholder="+ tag"
                className="w-24 bg-transparent py-1.5 text-sm outline-none placeholder:text-zinc-600"
              />
            </div>
            <button className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900">
              Adicionar
            </button>
          </form>

          <div className="space-y-1.5">
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
                  onApply={(m) => openPreview(m)}
                />
              ))
            )}
          </div>
        </div>
      )}

      {section === 'categories' && <CategoriesSection />}
      {section === 'tags' && <TagsSection />}
    </div>
  )
}
