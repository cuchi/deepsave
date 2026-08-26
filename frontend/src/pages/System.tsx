import { useQuery } from '@tanstack/react-query'
import { systemApi, type StatusCount, type TableCount } from '../api/client'

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n
  let u = -1
  do {
    v /= 1024
    u++
  } while (v >= 1024 && u < units.length - 1)
  return `${v.toLocaleString('pt-BR', { maximumFractionDigits: 1 })} ${units[u]}`
}

const STATUS_LABELS: Record<string, string> = {
  pending: 'Pendente',
  processing: 'Processando',
  processed: 'Processado',
  failed: 'Falhou',
  pending_review: 'Revisar',
  confirmed: 'Confirmado',
  rejected: 'Rejeitado',
  suggested: 'Sugerido',
}

function statusLabel(s: string): string {
  return STATUS_LABELS[s] ?? s
}

function Card({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-900 px-4 py-3">
      <p className="text-xs uppercase tracking-wide text-zinc-500">{label}</p>
      <p className="mt-1 text-2xl font-bold tabular-nums">{value}</p>
      {sub && <p className="mt-0.5 text-xs text-zinc-500">{sub}</p>}
    </div>
  )
}

function TableCounts({ rows }: { rows: TableCount[] }) {
  const total = rows.reduce((acc, r) => acc + r.count, 0)
  return (
    <div className="overflow-hidden rounded border border-zinc-800">
      <div className="flex items-center gap-3 bg-zinc-900 px-3 py-2 text-xs font-medium uppercase tracking-wide text-zinc-500">
        <span className="flex-1">Tabela</span>
        <span className="w-24 text-right">Linhas</span>
        <span className="w-28 text-right">Tamanho</span>
      </div>
      <ul className="divide-y divide-zinc-800">
        {rows.map((t) => (
          <li key={t.table} className="flex items-center gap-3 bg-zinc-950 px-3 py-2">
            <span className="min-w-0 flex-1 truncate font-mono text-sm">{t.table}</span>
            <span className="w-24 text-right text-sm tabular-nums text-zinc-300">
              {t.count.toLocaleString('pt-BR')}
            </span>
            <span className="w-28 text-right text-sm tabular-nums text-zinc-400">
              {fmtBytes(t.size_bytes)}
            </span>
          </li>
        ))}
        <li className="flex items-center gap-3 bg-zinc-900 px-3 py-2 text-sm font-semibold">
          <span className="flex-1">Total</span>
          <span className="w-24 text-right tabular-nums">{total.toLocaleString('pt-BR')}</span>
          <span className="w-28" />
        </li>
      </ul>
    </div>
  )
}

function StatusList({ title, rows }: { title: string; rows: StatusCount[] }) {
  const total = rows.reduce((acc, r) => acc + r.count, 0)
  return (
    <div className="rounded border border-zinc-800">
      <p className="border-b border-zinc-800 bg-zinc-900 px-3 py-2 text-xs font-medium uppercase tracking-wide text-zinc-500">
        {title} <span className="ml-1 text-zinc-600">({total})</span>
      </p>
      <ul className="divide-y divide-zinc-800">
        {rows.length === 0 ? (
          <li className="bg-zinc-950 px-3 py-2 text-sm text-zinc-500">—</li>
        ) : (
          rows.map((r) => (
            <li key={r.status} className="flex items-center gap-3 bg-zinc-950 px-3 py-2">
              <span className="flex-1 text-sm">{statusLabel(r.status)}</span>
              <span className="text-sm tabular-nums text-zinc-300">{r.count}</span>
            </li>
          ))
        )}
      </ul>
    </div>
  )
}

export default function System() {
  const { data, isLoading, isFetching, refetch } = useQuery({
    queryKey: ['system'],
    queryFn: systemApi.get,
    refetchInterval: 30_000,
  })

  return (
    <div>
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-xl font-bold">Sistema</h1>
        <button
          onClick={() => refetch()}
          disabled={isFetching}
          className="rounded bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 disabled:opacity-40"
        >
          {isFetching ? 'Atualizando…' : 'Atualizar'}
        </button>
        {data && (
          <span className="text-xs text-zinc-500">atualiza a cada 30s</span>
        )}
      </div>

      {isLoading || !data ? (
        <p className="text-sm text-zinc-500">carregando…</p>
      ) : (
        <div className="space-y-6">
          <div className="grid gap-4 sm:grid-cols-2">
            <Card
              label="Banco de dados (PostgreSQL)"
              value={fmtBytes(data.db_size_bytes)}
              sub="tabelas + índices do banco atual"
            />
            <Card
              label="Arquivos enviados (armazenamento)"
              value={fmtBytes(data.storage_size_bytes)}
              sub={`${data.storage_file_count} arquivo(s) em /app/storage`}
            />
          </div>

          <TableCounts rows={data.table_counts} />

          <div className="grid gap-6 sm:grid-cols-2">
            <StatusList title="Itens por status" rows={data.items_by_status} />
            <StatusList title="Documentos por status" rows={data.documents_by_status} />
          </div>
        </div>
      )}
    </div>
  )
}
