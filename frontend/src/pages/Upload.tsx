import { useCallback, useState } from 'react'
import { useDropzone } from 'react-dropzone'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { documentsApi, type DocumentKind } from '../api/client'
import type { DocumentSummary } from '../lib/types'

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

function DocRow({
  doc,
  onDelete,
  onReprocess,
}: {
  doc: DocumentSummary
  onDelete: (id: string) => void
  onReprocess: (id: string) => void
}) {
  const [open, setOpen] = useState(false)
  const { data } = useQuery({
    queryKey: ['document', doc.id],
    queryFn: () => documentsApi.get(doc.id),
    enabled: open,
  })

  return (
    <div className="rounded border border-zinc-800 bg-zinc-900">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-3 px-3 py-2 text-left text-sm"
      >
        <span className="font-medium">{doc.filename}</span>
        <span className="text-xs text-zinc-500">{KIND_LABELS[doc.kind as DocumentKind]}</span>
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
          // Images are always receipts, regardless of the dropdown selection.
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
      <h1 className="mb-4 text-xl font-bold">Upload de documentos</h1>

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
              onDelete={(id) => remove.mutate(id)}
              onReprocess={(id) => reprocess.mutate(id)}
            />
          ))
        )}
      </div>
    </div>
  )
}
