import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { categoriesApi } from '../api/client'

export default function Categories() {
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
      <h1 className="mb-4 text-xl font-bold">Categorias</h1>

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
            {c.color && (
              <span
                className="h-3 w-3 rounded-full"
                style={{ background: c.color }}
              />
            )}
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
