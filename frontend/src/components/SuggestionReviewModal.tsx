import { useEffect, useState } from 'react'
import type { SuggestionDetail } from '../lib/types'
import { fmtCents } from '../lib/format'

interface Props {
  suggestions: SuggestionDetail[]
  /** Open at this suggestion's index (defaults to the first). */
  startId?: string
  categories: { id: string; name: string }[]
  onApply: (id: string, opts: { tags?: string[]; category?: string }) => void
  onDismiss: (id: string) => void
  onClose: () => void
}

export default function SuggestionReviewModal({
  suggestions,
  startId,
  categories,
  onApply,
  onDismiss,
  onClose,
}: Props) {
  // Local review queue: entries leave it as they're applied/dismissed, so the
  // modal navigates independently of the (refreshed) parent list.
  const [queue, setQueue] = useState<SuggestionDetail[]>(suggestions)
  const [idx, setIdx] = useState(() => {
    const i = startId ? queue.findIndex((s) => s.id === startId) : 0
    return i >= 0 ? i : 0
  })
  const [applyCat, setApplyCat] = useState(true)
  const [applyTags, setApplyTags] = useState(true)
  /** '__suggested__' = AI proposal | '__none__' = clear | '__nova__' = type new
   *  | '__skip__' = don't touch | otherwise an existing category name. */
  const [catChoice, setCatChoice] = useState('__suggested__')
  const [newCatName, setNewCatName] = useState('')
  const [draftTags, setDraftTags] = useState<string[]>([])
  const [draftInput, setDraftInput] = useState('')
  const [busy, setBusy] = useState(false)

  const sug = queue[idx]
  const total = queue.length

  // Re-seed when the parent passes a new list (e.g. reopened) — but only if we
  // haven't consumed items yet.
  useEffect(() => {
    setQueue((q) => (q.length === 0 ? suggestions : q))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [suggestions])

  useEffect(() => {
    if (!sug) return
    setApplyCat(sug.suggested_category.trim() !== '')
    setApplyTags(sug.suggested_tags.length > 0)
    setCatChoice(sug.suggested_category.trim() !== '' ? '__suggested__' : '__skip__')
    setNewCatName('')
    setDraftTags([...sug.suggested_tags])
    setDraftInput('')
  }, [sug])

  const advance = () => {
    if (idx + 1 < queue.length) setIdx(idx + 1)
    else onClose()
  }
  const goPrev = () => {
    if (idx > 0) setIdx(idx - 1)
  }

  const removeCurrent = () => {
    const next = queue.filter((_, i) => i !== idx)
    setQueue(next)
    if (idx >= next.length) {
      if (next.length === 0) onClose()
      else setIdx(next.length - 1)
    }
  }

  const doApply = () => {
    if (!sug || busy) return
    setBusy(true)
    let catToSend: string | undefined
    if (applyCat) {
      if (catChoice === '__none__') catToSend = '__none__'
      else if (catChoice === '__nova__') {
        const n = newCatName.trim()
        catToSend = n ? `nova: ${n}` : undefined
      } else if (catChoice === '__suggested__') {
        catToSend = sug.suggested_category.trim() !== '' ? sug.suggested_category : undefined
      } else if (catChoice !== '__skip__') {
        catToSend = catChoice
      }
    }
    onApply(sug.id, {
      tags: applyTags ? draftTags : undefined,
      category: catToSend,
    })
    // The parent mutation refetches; optimistically move on after a tick.
    setTimeout(() => {
      setBusy(false)
      removeCurrent()
    }, 350)
  }
  const doDismiss = () => {
    if (!sug || busy) return
    setBusy(true)
    onDismiss(sug.id)
    setTimeout(() => {
      setBusy(false)
      removeCurrent()
    }, 350)
  }

  // Keyboard: ←/→ navigate, Enter applies, Esc closes (but not while typing).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) {
        if (e.key === 'Escape') onClose()
        return
      }
      if (e.key === 'ArrowRight') advance()
      else if (e.key === 'ArrowLeft') goPrev()
      else if (e.key === 'Enter') doApply()
      else if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [idx, queue, applyCat, applyTags, draftTags, busy])

  const fmtDate = (iso: string) => {
    const d = iso.slice(0, 10).split('-')
    return `${d[2]}/${d[1]}/${d[0]}`
  }

  const item = sug
  const newCategory = item?.suggested_category.startsWith('nova:')

  if (!item) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="fixed inset-0 bg-black/70" onClick={onClose} />
      <div className="relative w-full max-w-lg rounded-xl border border-zinc-700 bg-zinc-900 shadow-2xl">
        {/* Header: counter + close */}
        <div className="flex items-center justify-between border-b border-zinc-800 px-4 py-2.5">
          <p className="text-xs text-zinc-500">
            Revisão de sugestões · <span className="text-zinc-300">{idx + 1}/{total}</span>
          </p>
          <button onClick={onClose} className="text-xs text-zinc-500 hover:text-zinc-200">
            Esc ✕
          </button>
        </div>

        {/* Item context */}
        <div className="px-4 py-3">
          <p className="text-sm font-medium text-zinc-100">
            {item.merchant || item.description}
          </p>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-zinc-500">
            {item.merchant && item.description && (
              <span className="truncate">{item.description}</span>
            )}
            <span className="tabular-nums">{fmtCents(item.amount_cents)}</span>
            <span>{fmtDate(item.occurred_on)}</span>
          </div>
          {(item.pluggy_category || item.mcc || item.operation_type || item.payment_method) && (
            <div className="mt-1.5 flex flex-wrap gap-1.5 text-[10px] text-zinc-500">
              {item.pluggy_category && (
                <span className="rounded bg-zinc-800 px-1.5 py-0.5">pc: {item.pluggy_category}</span>
              )}
              {item.mcc != null && (
                <span className="rounded bg-zinc-800 px-1.5 py-0.5">mcc: {item.mcc}</span>
              )}
              {item.operation_type && (
                <span className="rounded bg-zinc-800 px-1.5 py-0.5">op: {item.operation_type}</span>
              )}
              {item.payment_method && (
                <span className="rounded bg-zinc-800 px-1.5 py-0.5">pay: {item.payment_method}</span>
              )}
            </div>
          )}
          <div className="mt-1.5 text-xs text-zinc-500">
            atual:{' '}
            {item.category_name ? (
              <span className="text-zinc-300">{item.category_name}</span>
            ) : (
              <span className="text-zinc-600">sem categoria</span>
            )}
            {item.tags.length > 0 && (
              <span className="ml-2">
                tags:{' '}
                <span className="text-zinc-300">
                  {item.tags.map((t) => `#${t}`).join(' ')}
                </span>
              </span>
            )}
          </div>
        </div>

        {/* Category proposal */}
        <div className="border-t border-zinc-800 px-4 py-3">
          <div className="mb-1.5 flex items-center gap-2">
            <input
              type="checkbox"
              checked={applyCat}
              disabled={item.suggested_category.trim() === ''}
              onChange={(e) => setApplyCat(e.target.checked)}
              className="accent-amber-300"
            />
            <span className="text-xs font-medium text-zinc-400">Categoria sugerida</span>
            {newCategory && (
              <span className="rounded bg-emerald-950 px-1.5 py-0.5 text-[10px] text-emerald-300">
                nova categoria
              </span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-1.5 pl-6">
            <select
              value={catChoice}
              onChange={(e) => {
                setCatChoice(e.target.value)
                if (e.target.value !== '__skip__' && e.target.value !== '__suggested__') {
                  setApplyCat(true)
                }
              }}
              className="max-w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm text-zinc-200 outline-none focus:border-zinc-500"
            >
              {item.suggested_category.trim() !== '' && (
                <option value="__suggested__">✨ {item.suggested_category}</option>
              )}
              {categories.map((c) => (
                <option key={c.id} value={c.name}>
                  {c.name}
                </option>
              ))}
              <option value="__nova__">➕ nova categoria…</option>
              <option value="__none__">— sem categoria —</option>
              <option value="__skip__">— não mexer —</option>
            </select>
            {catChoice === '__nova__' && (
              <input
                value={newCatName}
                onChange={(e) => setNewCatName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    e.stopPropagation()
                  }
                }}
                placeholder="nome da nova categoria"
                autoFocus
                className="w-44 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm outline-none focus:border-zinc-500"
              />
            )}
          </div>
          {catChoice === '__suggested__' && item.suggested_category.trim() === '' && (
            <p className="pl-6 pt-1 text-xs text-zinc-600">
              sem sugestão (movimento de dinheiro — transferência, fatura…)
            </p>
          )}
        </div>

        {/* Tags proposal */}
        <div className="border-t border-zinc-800 px-4 py-3">
          <div className="mb-1.5 flex items-center gap-2">
            <input
              type="checkbox"
              checked={applyTags}
              onChange={(e) => setApplyTags(e.target.checked)}
              className="accent-amber-300"
            />
            <span className="text-xs font-medium text-zinc-400">Tags sugeridas</span>
          </div>
          <div className="flex flex-wrap gap-1 pl-6">
            {draftTags.length === 0 ? (
              <span className="text-xs text-zinc-600">nenhuma</span>
            ) : (
              draftTags.map((t) => (
                <span
                  key={t}
                  className="flex items-center gap-1 rounded border border-amber-500/50 bg-amber-500/10 px-1.5 py-0.5 text-[11px] text-amber-300"
                >
                  {t}
                  <button
                    onClick={() => setDraftTags((d) => d.filter((x) => x !== t))}
                    className="text-amber-500/70 hover:text-amber-200"
                  >
                    ×
                  </button>
                </span>
              ))
            )}
            <input
              value={draftInput}
              onChange={(e) => setDraftInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  e.stopPropagation()
                  const t = draftInput.trim().toLowerCase()
                  if (t && !draftTags.includes(t)) {
                    setDraftTags((d) => [...d, t])
                    setApplyTags(true)
                  }
                  setDraftInput('')
                }
              }}
              placeholder="+ tag"
              className="w-24 rounded border border-zinc-700 bg-zinc-950 px-2 py-0.5 text-xs outline-none focus:border-zinc-500"
            />
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-2 border-t border-zinc-800 px-4 py-2.5">
          <div className="flex items-center gap-1 text-zinc-500">
            <button
              onClick={goPrev}
              disabled={idx === 0}
              className="rounded border border-zinc-700 px-2 py-1 text-xs hover:border-zinc-500 disabled:opacity-30"
            >
              ←
            </button>
            <button
              onClick={advance}
              disabled={idx + 1 >= queue.length}
              className="rounded border border-zinc-700 px-2 py-1 text-xs hover:border-zinc-500 disabled:opacity-30"
            >
              →
            </button>
            <span className="ml-1 hidden text-[10px] text-zinc-600 sm:inline">setas navegam</span>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={doDismiss}
              disabled={busy}
              className="rounded border border-zinc-700 px-2.5 py-1 text-xs text-zinc-400 hover:border-zinc-500 disabled:opacity-50"
            >
              Ignorar
            </button>
            <button
              onClick={doApply}
              disabled={busy || (!applyCat && !applyTags)}
              className="rounded bg-amber-300 px-3 py-1 text-xs font-semibold text-amber-950 hover:bg-amber-200 disabled:opacity-50"
            >
              Aplicar
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
