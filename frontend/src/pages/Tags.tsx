import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { tagsApi } from '../api/client'

export default function Tags() {
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
  const remove = useMutation({
    mutationFn: tagsApi.remove,
    onSuccess: invalidate,
  })

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
      <h1 className="mb-1 text-xl font-bold">Tags</h1>
      <p className="mb-4 text-sm text-zinc-500">
        Renomear, mesclar ou apagar tags — as alterações valem para todos os itens.
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
              <li
                key={u.tag}
                className="flex flex-wrap items-center gap-3 bg-zinc-950 px-3 py-2"
              >
                <span className="min-w-0 flex-1 truncate font-mono text-sm">{u.tag}</span>
                <span className="w-16 text-right text-sm tabular-nums text-zinc-400">
                  {u.count}
                </span>

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
                    <button
                      onClick={() => setRenaming(null)}
                      className="text-xs text-zinc-500 hover:text-zinc-200"
                    >
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
                    <button
                      onClick={() => setMerging(null)}
                      className="text-xs text-zinc-500 hover:text-zinc-200"
                    >
                      cancelar
                    </button>
                  </div>
                ) : (
                  <div className="flex w-60 items-center justify-end gap-3 text-xs">
                    <button
                      onClick={() => startRename(u.tag)}
                      className="text-zinc-400 hover:text-zinc-100"
                    >
                      renomear
                    </button>
                    <button
                      onClick={() => startMerge(u.tag)}
                      className="text-zinc-400 hover:text-zinc-100"
                    >
                      mesclar
                    </button>
                    <button
                      onClick={() => doDelete(u.tag)}
                      className="text-zinc-500 hover:text-red-400"
                    >
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
