import { FormEvent, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { categoriesApi, itemsApi, tagsApi, type ItemInput } from '../api/client'
import type { Item } from '../lib/types'
import { fmtCents } from '../lib/format'

const KIND_OPTIONS: [string, string][] = [
  ['expense', 'Despesa'],
  ['income', 'Receita'],
  ['refund', 'Estorno'],
  ['internal', 'Interna'],
]

interface Props {
  month: string
  parent?: Item | null
  editing?: Item | null
  onClose: () => void
}

export default function ItemForm({ month, parent, editing, onClose }: Props) {
  const { data: categories = [] } = useQuery({
    queryKey: ['categories'],
    queryFn: categoriesApi.list,
  })
  const { data: allTags = [] } = useQuery({
    queryKey: ['tags'],
    queryFn: tagsApi.list,
  })

  const isEditing = !!editing

  const [description, setDescription] = useState(editing?.description ?? '')
  const [amount, setAmount] = useState(
    editing ? String(Math.abs(editing.amount_cents) / 100) : '',
  )
  const [date, setDate] = useState(editing?.occurred_on ?? `${month}-01`)
  const [kind, setKind] = useState(editing?.kind ?? 'expense')
  const [merchant, setMerchant] = useState(editing?.merchant ?? '')
  const [categoryId, setCategoryId] = useState(editing?.category_id ?? '')
  const [tags, setTags] = useState<string[]>(editing?.tags ?? [])
  const [tagInput, setTagInput] = useState('')
  const [updateMemory, setUpdateMemory] = useState(true)
  const [busy, setBusy] = useState(false)

  const addTag = () => {
    const t = tagInput.trim().replace(/,$/, '')
    if (t && !tags.includes(t)) {
      setTags([...tags, t])
    }
    setTagInput('')
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)

    let input: ItemInput
    if (editing) {
      // Category, tags and kind are editable on an existing item; the rest is fixed.
      input = {
        parent_id: editing.parent_id,
        kind,
        account_id: editing.account_id,
        installment: editing.installment,
        installment_count: editing.installment_count,
        occurred_on: editing.occurred_on,
        merchant: editing.merchant,
        description: editing.description,
        amount_cents: editing.amount_cents,
        currency: editing.currency,
        category_id: categoryId || null,
        tags,
        update_memory: updateMemory,
      }
    } else {
      const parsed = parseFloat(amount.replace(',', '.'))
      if (Number.isNaN(parsed)) {
        setBusy(false)
        return
      }
      const amountCents = Math.round(parsed * 100)
      const signed = kind === 'expense' ? -Math.abs(amountCents) : Math.abs(amountCents)
      input = {
        parent_id: parent?.id ?? null,
        kind,
        occurred_on: date,
        merchant: merchant || null,
        description,
        amount_cents: signed,
        category_id: categoryId || null,
        tags,
      }
    }

    try {
      if (editing) {
        await itemsApi.update(editing.id, input)
      } else {
        await itemsApi.create(input)
      }
      onClose()
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
          {isEditing
            ? 'Editar item'
            : parent
              ? `Sub-item de “${parent.description}”`
              : 'Novo item'}
        </h2>

        {isEditing ? (
          <>
            <div className="rounded border border-zinc-800 bg-zinc-950 px-3 py-2">
              <p className="text-sm font-medium">{editing!.description}</p>
              <p className="text-xs text-zinc-500">
                {editing!.occurred_on} · {fmtCents(editing!.amount_cents)}
                {editing!.merchant ? ` · ${editing!.merchant}` : ''}
              </p>
            </div>

            <select
              value={kind}
              onChange={(e) => setKind(e.target.value)}
              className="field"
            >
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
              <option value="">Sem categoria</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
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
                list="tags-list"
                className="min-w-[7rem] flex-1 bg-transparent text-sm outline-none"
              />
              <datalist id="tags-list">
                {allTags.map((t) => (
                  <option key={t} value={t} />
                ))}
              </datalist>
            </div>

            <label className="flex items-center gap-2 text-xs text-zinc-400">
              <input
                type="checkbox"
                checked={updateMemory}
                onChange={(e) => setUpdateMemory(e.target.checked)}
                disabled={!editing!.merchant}
                className="checkbox"
              />
              Salvar na memória (categoria + tags deste comerciante)
            </label>
          </>
        ) : (
          <>
            <input
              required
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Descrição"
              className="field"
            />

            <div className="grid grid-cols-2 gap-3">
              <input
                required
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                placeholder="Valor (R$)"
                inputMode="decimal"
                className="field"
              />
              <input
                required
                type="date"
                value={date}
                onChange={(e) => setDate(e.target.value)}
                className="field"
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <select
                value={kind}
                onChange={(e) => setKind(e.target.value)}
                className="field"
              >
                <option value="expense">Despesa</option>
                <option value="income">Receita</option>
                <option value="refund">Estorno</option>
                <option value="internal">Interna</option>
              </select>
              <input
                value={merchant}
                onChange={(e) => setMerchant(e.target.value)}
                placeholder="Comerciante"
                className="field"
              />
            </div>

            <select
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              className="field"
            >
              <option value="">Sem categoria</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
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
                list="tags-list"
                className="min-w-[7rem] flex-1 bg-transparent text-sm outline-none"
              />
              <datalist id="tags-list">
                {allTags.map((t) => (
                  <option key={t} value={t} />
                ))}
              </datalist>
            </div>
          </>
        )}

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
            Salvar
          </button>
        </div>
      </form>
    </div>
  )
}
