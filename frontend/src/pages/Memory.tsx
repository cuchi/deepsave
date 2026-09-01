import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  categoriesApi,
  changeLogApi,
  diaryApi,
  tagsApi,
  type ChangeLogEntry,
  type DiaryEntry,
} from '../api/client'
import { fmtCents } from '../lib/format'

type Section = 'categories' | 'tags' | 'history' | 'diary'

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

// ---------------------------------------------------------------------------
// Histórico section — durable log of category/tag changes (feeds the AI)
// ---------------------------------------------------------------------------

const SOURCE_LABELS: Record<string, string> = {
  item_edit: 'Edição',
  bulk: 'Em massa',
  memory_apply: 'Memória',
  ai_apply: 'IA',
}

function HistorySection() {
  const [merchant, setMerchant] = useState('')
  const [source, setSource] = useState('')
  const { data: entries = [] } = useQuery({
    queryKey: ['change-log', merchant, source],
    queryFn: () => changeLogApi.list({ merchant: merchant || undefined, source: source || undefined }),
  })

  const fmtDate = (iso: string) => {
    const d = new Date(iso)
    return d.toLocaleDateString('pt-BR') + ' ' + d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
  }

  return (
    <div>
      <p className="mb-4 text-sm text-zinc-500">
        Log das suas mudanças de categoria e tags — é disso que a IA aprende.
      </p>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <input
          value={merchant}
          onChange={(e) => setMerchant(e.target.value)}
          placeholder="Filtrar por loja/comerciante…"
          className="field w-56 py-1 text-xs"
        />
        <select value={source} onChange={(e) => setSource(e.target.value)} className="field w-40 py-1 text-xs">
          <option value="">Todas as origens</option>
          {Object.entries(SOURCE_LABELS).map(([k, v]) => (
            <option key={k} value={k}>{v}</option>
          ))}
        </select>
      </div>

      {entries.length === 0 ? (
        <p className="text-sm text-zinc-500">Nenhuma mudança registrada ainda.</p>
      ) : (
        <ul className="divide-y divide-zinc-800 overflow-hidden rounded border border-zinc-800">
          {entries.map((e: ChangeLogEntry, i: number) => {
            const catChanged = e.category_before !== e.category_after
            const tagsChanged = JSON.stringify(e.tags_before) !== JSON.stringify(e.tags_after)
            return (
              <li key={`${e.created_at}-${i}-${e.merchant_key}`} className="bg-zinc-950 px-3 py-2 text-sm">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="text-xs tabular-nums text-zinc-500">{fmtDate(e.created_at)}</span>
                  {e.tx_date && (
                    <span className="text-xs tabular-nums text-zinc-600">compra {e.tx_date.slice(8, 10)}/{e.tx_date.slice(5, 7)}/{e.tx_date.slice(0, 4)}</span>
                  )}
                  <span className="min-w-0 flex-1 truncate text-zinc-300">{e.merchant ?? e.description}</span>
                  {e.amount_cents != null && (
                    <span className={`text-xs tabular-nums ${e.amount_cents < 0 ? 'text-zinc-300' : 'text-emerald-400'}`}>
                      {fmtCents(e.amount_cents)}
                    </span>
                  )}
                  {e.bank && (
                    <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">{e.bank}</span>
                  )}
                  <span className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
                    {SOURCE_LABELS[e.source] ?? e.source}
                  </span>
                </div>
                <div className="mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-xs text-zinc-400">
                  {catChanged && (
                    <span>
                      Categoria: {e.category_before ?? '—'} <span className="text-zinc-600">→</span>{' '}
                      {e.category_after ?? '—'}
                    </span>
                  )}
                  {tagsChanged && (
                    <span>
                      Tags:{' '}
                      {e.tags_before.length === 0
                        ? '—'
                        : e.tags_before.join(', ')}{' '}
                      <span className="text-zinc-600">→</span>{' '}
                      {e.tags_after.length === 0 ? '—' : e.tags_after.join(', ')}
                    </span>
                  )}
                  {!catChanged && !tagsChanged && e.current_category && (
                    <span>Categoria atual: {e.current_category}</span>
                  )}
                  {(e.mcc || e.pluggy_category || e.operation_type) && (
                    <span className="text-zinc-600">
                      {e.mcc && `MCC ${e.mcc}`}
                      {e.mcc && (e.pluggy_category || e.operation_type) ? ' · ' : ''}
                      {e.pluggy_category && e.pluggy_category}
                      {e.pluggy_category && e.operation_type ? ' · ' : ''}
                      {e.operation_type && e.operation_type}
                    </span>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

// Tags section (folded-in)
// ---------------------------------------------------------------------------


function DiarySection() {
  const qc = useQueryClient()
  const [date, setDate] = useState('')
  const [comment, setComment] = useState('')
  const [editing, setEditing] = useState<DiaryEntry | null>(null)
  const [editDate, setEditDate] = useState('')
  const [editComment, setEditComment] = useState('')

  const { data: entries = [] } = useQuery({ queryKey: ['diary'], queryFn: diaryApi.list })
  const invalidate = () => qc.invalidateQueries({ queryKey: ['diary'] })

  const create = useMutation({
    mutationFn: (v: { entry_date: string; comment: string }) => diaryApi.create(v),
    onSuccess: () => {
      invalidate()
      setDate('')
      setComment('')
    },
  })
  const update = useMutation({
    mutationFn: (v: { id: string; entry_date: string; comment: string }) => diaryApi.update(v.id, v),
    onSuccess: () => {
      setEditing(null)
      invalidate()
    },
  })
  const remove = useMutation({ mutationFn: diaryApi.remove, onSuccess: invalidate })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!date || !comment.trim()) return
    create.mutate({ entry_date: date, comment: comment.trim() })
  }

  const fmtDate = (iso: string) => `${iso.slice(8, 10)}/${iso.slice(5, 7)}/${iso.slice(0, 4)}`

  return (
    <div>
      <p className="mb-4 text-sm text-zinc-500">
        Anotações sobre sua vida — a IA usa isso para entender o contexto dos seus gastos.
        Ex.: <code>2025-09-01 · Mudei de cidade</code>
      </p>

      <form onSubmit={submit} className="mb-6 flex flex-wrap gap-2">
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} required className="field w-40 py-1 text-xs" />
        <input value={comment} onChange={(e) => setComment(e.target.value)} placeholder="O que aconteceu?" className="field min-w-[16rem] flex-1 py-1 text-sm" />
        <button className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900">Adicionar</button>
      </form>

      {entries.length === 0 ? (
        <p className="text-sm text-zinc-500">Nenhuma anotação ainda.</p>
      ) : (
        <ul className="divide-y divide-zinc-800 overflow-hidden rounded border border-zinc-800">
          {entries.map((e) =>
            editing?.id === e.id ? (
              <li key={e.id} className="flex flex-wrap items-center gap-2 bg-zinc-950 px-3 py-2">
                <input type="date" value={editDate} onChange={(ev) => setEditDate(ev.target.value)} className="field w-40 py-1 text-xs" />
                <input value={editComment} onChange={(ev) => setEditComment(ev.target.value)} className="field min-w-[12rem] flex-1 py-1 text-sm" />
                <button
                  onClick={() => editDate && editComment.trim() && update.mutate({ id: e.id, entry_date: editDate, comment: editComment.trim() })}
                  className="rounded bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-900"
                >
                  salvar
                </button>
                <button onClick={() => setEditing(null)} className="text-xs text-zinc-500 hover:text-zinc-200">cancelar</button>
              </li>
            ) : (
              <li key={e.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-zinc-950 px-3 py-2 text-sm">
                <span className="shrink-0 text-xs tabular-nums text-zinc-500">{fmtDate(e.entry_date)}</span>
                <span className="min-w-0 flex-1 text-zinc-300">{e.comment}</span>
                <button
                  onClick={() => { setEditing(e); setEditDate(e.entry_date); setEditComment(e.comment) }}
                  className="text-xs text-zinc-500 hover:text-zinc-200"
                >
                  editar
                </button>
                <button
                  onClick={() => window.confirm('Apagar esta anotação?') && remove.mutate(e.id)}
                  className="text-xs text-zinc-500 hover:text-red-400"
                >
                  apagar
                </button>
              </li>
            ),
          )}
        </ul>
      )}
    </div>
  )
}

function TagsSection() {
  const qc = useQueryClient()
  const { data: usage = [] } = useQuery({
    queryKey: ['tags-usage'],
    queryFn: tagsApi.usage,
  })
  const { data: registry = [] } = useQuery({
    queryKey: ['tags-registry'],
    queryFn: tagsApi.registry,
  })
  const [descriptions, setDescriptions] = useState<Record<string, string>>({})

  const [renaming, setRenaming] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [merging, setMerging] = useState<string | null>(null)
  const [mergeInto, setMergeInto] = useState('')

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['tags-usage'] })
    qc.invalidateQueries({ queryKey: ['tags-registry'] })
    qc.invalidateQueries({ queryKey: ['tags'] })
    qc.invalidateQueries({ queryKey: ['items'] })
  }

  const saveDesc = useMutation({
    mutationFn: (v: { tag: string; description: string }) =>
      tagsApi.setDescription(v.tag, v.description),
    onSuccess: invalidate,
  })
  const descByTag = new Map(registry.map((r) => [r.name, r.description]))

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
              <li key={u.tag} className="bg-zinc-950 px-3 py-2">
                <div className="flex flex-wrap items-center gap-3">
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
                </div>
                <input
                  value={descriptions[u.tag] ?? descByTag.get(u.tag) ?? ''}
                  onChange={(e) => setDescriptions((d) => ({ ...d, [u.tag]: e.target.value }))}
                  onBlur={() => {
                    const v = (descriptions[u.tag] ?? descByTag.get(u.tag) ?? '').trim()
                    if (v !== (descByTag.get(u.tag) ?? '').trim()) saveDesc.mutate({ tag: u.tag, description: v })
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                  }}
                  placeholder="O que essa tag significa? (a IA usa isso)"
                  className="mt-1.5 w-full rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs text-zinc-400 placeholder:text-zinc-600 focus:border-zinc-600 focus:outline-none"
                />
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
  { id: 'categories', label: 'Categorias' },
  { id: 'tags', label: 'Tags' },
  { id: 'history', label: 'Histórico' },
  { id: 'diary', label: 'Diário' },
]

export default function Memory() {
  const [section, setSection] = useState<Section>('categories')

  return (
    <div>
      <div className="mb-4 flex items-center gap-3">
        <h1 className="text-xl font-bold">Memória</h1>
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

      {section === 'categories' && <CategoriesSection />}
      {section === 'tags' && <TagsSection />}
      {section === 'history' && <HistorySection />}
      {section === 'diary' && <DiarySection />}
    </div>
  )
}
