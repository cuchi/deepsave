import { useCallback, useState } from 'react'
import { useDropzone } from 'react-dropzone'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { coverageApi, documentsApi, sourcesApi, type DocumentKind } from '../api/client'
import type { DocumentSummary } from '../lib/types'
import BankLogo from '../components/BankLogo'

const KIND_LABELS: Record<DocumentKind, string> = {
  card_statement: 'Fatura de cartão',
  bank_statement: 'Extrato bancário',
  receipt: 'Recibo / nota',
  payment_slip: 'Boleto / comprovante',
}

const STATUS_STYLES: Record<string, string> = {
  pending: 'text-zinc-400',
  processing: 'text-amber-400',
  needs_review: 'text-amber-400',
  processed: 'text-emerald-400',
  failed: 'text-red-400',
}

function fmtDate(d: string): string {
  return `${d.slice(8, 10)}/${d.slice(5, 7)}/${d.slice(0, 4)}`
}

function dateRange(doc: DocumentSummary): string {
  if (!doc.first_date && !doc.last_date) return ''
  const first = doc.first_date ?? doc.last_date!
  const last = doc.last_date ?? doc.first_date!
  return first === last ? fmtDate(first) : `${fmtDate(first)} → ${fmtDate(last)}`
}

function DocRow({
  doc,
  bank,
  onDelete,
  onReprocess,
}: {
  doc: DocumentSummary
  bank?: string | null
  onDelete: (id: string) => void
  onReprocess: (id: string) => void
}) {
  const [open, setOpen] = useState(false)
  const { data } = useQuery({
    queryKey: ['document', doc.id],
    queryFn: () => documentsApi.get(doc.id),
    enabled: open,
  })

  const range = dateRange(doc)

  return (
    <div className="rounded border border-zinc-800 bg-zinc-900">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2 text-left text-sm"
      >
        <BankLogo bank={bank} />
        <span className="font-medium">{KIND_LABELS[doc.kind as DocumentKind]}</span>
        {range && <span className="text-xs tabular-nums text-zinc-500">{range}</span>}
        <span className="text-xs text-zinc-500">{doc.item_count} itens</span>
        <span className={`ml-auto text-xs ${STATUS_STYLES[doc.status] ?? 'text-zinc-400'}`}>
          {doc.status}
        </span>
        <span
          role="button"
          tabIndex={0}
          onClick={(e) => {
            e.stopPropagation()
            onReprocess(doc.id)
          }}
          className="text-xs text-zinc-500 hover:text-zinc-200"
        >
          reprocessar
        </span>
        <span
          role="button"
          tabIndex={0}
          onClick={(e) => {
            e.stopPropagation()
            onDelete(doc.id)
          }}
          className="text-xs text-zinc-500 hover:text-red-400"
        >
          apagar
        </span>
      </button>

      {open && (
        <div className="border-t border-zinc-800 p-3">
          <p className="mb-2 text-xs text-zinc-400">
            <span className="text-zinc-300">{doc.filename}</span>
          </p>
          {doc.error_message && (
            <p className="mb-2 text-xs text-red-400">{doc.error_message}</p>
          )}
          {data?.ocr_text ? (
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded bg-zinc-950 p-3 text-xs text-zinc-300">
              {data.ocr_text}
            </pre>
          ) : (
            <p className="text-xs text-zinc-500">
              {data && data.items.length > 0
                ? `${data.items.length} itens extraídos deste documento.`
                : 'Sem texto extraído.'}
            </p>
          )}
        </div>
      )}
    </div>
  )
}

export default function Upload() {
  const [kind, setKind] = useState<DocumentKind>('card_statement')
  const [uploading, setUploading] = useState(false)
  const qc = useQueryClient()

  const { data: docs = [] } = useQuery({
    queryKey: ['documents'],
    queryFn: documentsApi.list,
    refetchInterval: 3000,
  })
  const { data: sources = [] } = useQuery({ queryKey: ['sources'], queryFn: sourcesApi.list })

  const { data: coverage } = useQuery({ queryKey: ['coverage'], queryFn: coverageApi.get })
  const toggleSource = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      sourcesApi.update(id, { enabled }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['coverage'] }),
  })

  const bankBySource = new Map(sources.map((s) => [s.id, s.bank]))

  const remove = useMutation({
    mutationFn: documentsApi.remove,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  })
  const reprocess = useMutation({
    mutationFn: documentsApi.reprocess,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['documents'] }),
  })

  const onDrop = useCallback(
    async (files: File[]) => {
      for (const file of files) {
        setUploading(true)
        try {
          const effectiveKind: DocumentKind = file.type.startsWith('image/')
            ? 'receipt'
            : kind
          await documentsApi.upload(effectiveKind, file)
          qc.invalidateQueries({ queryKey: ['documents'] })
        } catch (e) {
          console.error('upload failed', e)
        } finally {
          setUploading(false)
        }
      }
    },
    [kind, qc],
  )

  const { getRootProps, getInputProps, isDragActive } = useDropzone({ onDrop })

  return (
    <div>
      <h1 className="mb-4 text-xl font-bold">Documentos</h1>

      <div className="mb-6 flex items-center gap-3">
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as DocumentKind)}
          className="field w-56"
        >
          {(Object.keys(KIND_LABELS) as DocumentKind[]).map((k) => (
            <option key={k} value={k}>
              {KIND_LABELS[k]}
            </option>
          ))}
        </select>
      </div>

      <div
        {...getRootProps()}
        className={`mb-6 grid cursor-pointer place-items-center rounded-lg border-2 border-dashed p-10 text-sm text-zinc-400 transition-colors ${
          isDragActive ? 'border-zinc-400 bg-zinc-900' : 'border-zinc-800 hover:border-zinc-600'
        }`}
      >
        <input {...getInputProps()} />
        {uploading
          ? 'Enviando…'
          : 'Arraste arquivos aqui, ou clique para selecionar (PDF, CSV, JPG, PNG)'}
      </div>

      <div className="space-y-2">
        {docs.length === 0 ? (
          <p className="text-sm text-zinc-500">Nenhum documento enviado.</p>
        ) : (
          docs.map((d) => (
            <DocRow
              key={d.id}
              doc={d}
              bank={d.source_id ? bankBySource.get(d.source_id) : undefined}
              onDelete={(id) => remove.mutate(id)}
              onReprocess={(id) => reprocess.mutate(id)}
            />
          ))
        )}
      </div>

      {/* Coverage */}
      <h2 className="mb-2 mt-10 text-lg font-semibold">Cobertura de fontes</h2>
      <p className="mb-4 text-sm text-zinc-500">
        Fontes fundamentais (bancos × conta/cartão) e os últimos {(coverage?.months ?? []).length}{' '}
        meses. <span className="text-emerald-400">●</span> presente ·{' '}
        <span className="text-amber-400">◐</span> parcial · <span className="text-zinc-700">●</span> ausente.
      </p>

      <div className="overflow-x-auto rounded border border-zinc-800">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-zinc-800">
              <th className="py-2 pl-4 pr-4 text-left font-medium text-zinc-400">Fonte</th>
              {(coverage?.months ?? []).map((m) => (
                <th
                  key={m}
                  className="px-1 py-2 text-center text-[10px] font-normal text-zinc-500"
                >
                  {`${m.slice(5)}/${m.slice(2, 4)}`}
                </th>
              ))}
              <th className="py-2 pl-4 pr-4 text-left font-medium text-zinc-400">Último envio</th>
            </tr>
          </thead>
          <tbody>
            {(coverage?.sources ?? []).map((s) => {
              const present = new Set(s.present)
              const partial = new Set(s.partial ?? [])
              return (
                <tr key={s.id} className={s.enabled ? '' : 'opacity-40'}>
                  <td className="py-2 pl-4 pr-4">
                    <span className="font-medium">{s.name}</span>
                    <button
                      onClick={() => toggleSource.mutate({ id: s.id, enabled: !s.enabled })}
                      className="ml-2 text-xs text-zinc-500 hover:text-zinc-200"
                    >
                      {s.enabled ? 'desativar' : 'ativar'}
                    </button>
                  </td>
                  {(coverage?.months ?? []).map((m) => (
                    <td key={m} className="px-1 py-2 text-center">
                      {present.has(m) ? (
                        <span className="text-emerald-400" title="Mês completo">
                          ●
                        </span>
                      ) : partial.has(m) ? (
                        <span className="text-amber-400" title="Mês parcial — extrato não cobre o mês todo">
                          ◐
                        </span>
                      ) : (
                        <span className="text-zinc-700">●</span>
                      )}
                    </td>
                  ))}
                  <td className="py-2 pl-4 pr-4 text-xs text-zinc-500">
                    {s.last_seen ? new Date(s.last_seen).toLocaleDateString('pt-BR') : 'nunca'}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </div>
  )
}
