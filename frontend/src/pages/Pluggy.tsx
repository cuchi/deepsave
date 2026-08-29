import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { pluggyApi } from '../api/client'
import type { PluggyAccount, PluggySyncResult } from '../lib/types'

const BANK_LABELS: Record<string, string> = {
  nubank: 'Nubank',
  caixa: 'Caixa',
  c6: 'C6',
}

function bankLabel(bank: string | null): string {
  return bank ? (BANK_LABELS[bank] ?? bank) : 'Pluggy'
}

function fmtWhen(iso: string | null | undefined): string {
  if (!iso) return 'nunca'
  const d = new Date(iso)
  return (
    d.toLocaleDateString('pt-BR') + ' ' + d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
  )
}

function fmtDate(iso: string | null): string {
  if (!iso) return '—'
  return `${iso.slice(8, 10)}/${iso.slice(5, 7)}/${iso.slice(0, 4)}`
}

function AccountRow({ a }: { a: PluggyAccount }) {
  const isCard = a.account_type === 'CREDIT'
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm">
      <span className="min-w-0 flex-1 truncate text-zinc-300">{a.name}</span>
      <span
        className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
          isCard ? 'bg-zinc-800 text-zinc-300' : 'bg-emerald-950 text-emerald-300'
        }`}
      >
        {isCard ? 'Cartão' : 'Conta'}
      </span>
      <span className="hidden text-xs text-zinc-600 sm:inline">
        {a.first_date ? `${fmtDate(a.first_date)} → ${fmtDate(a.last_date)}` : 'sem itens'}
      </span>
      <span className="text-xs tabular-nums text-zinc-400">{a.item_count} itens</span>
      <span className="text-xs text-zinc-500">sync {fmtWhen(a.last_sync_at)}</span>
    </li>
  )
}

export default function Pluggy() {
  const qc = useQueryClient()
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const { data: status } = useQuery({ queryKey: ['pluggy-status'], queryFn: pluggyApi.status })
  const { data: accounts } = useQuery({
    queryKey: ['pluggy-accounts'],
    queryFn: pluggyApi.accounts,
    enabled: status?.configured,
  })

  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')

  const syncMutation = useMutation({
    mutationFn: () => pluggyApi.sync(fromDate || undefined, toDate || undefined),
    onSuccess: (r: PluggySyncResult) => {
      const total = (r.accounts ?? []).reduce((acc, a) => acc + (a.new ?? 0), 0)
      setMessage(
        `Sincronização concluída: ${total} novo(s) importado(s).`,
      )
      setError(null)
      qc.invalidateQueries({ queryKey: ['pluggy-accounts'] })
      qc.invalidateQueries({ queryKey: ['pluggy-status'] })
      qc.invalidateQueries({ queryKey: ['items'] })
      qc.invalidateQueries({ queryKey: ['dashboard'] })
    },
    onError: (e: { message?: string }) => setError(`Falha na sincronização: ${e.message ?? 'erro desconhecido'}`),
  })

  if (!status?.configured) {
    return (
      <div className="space-y-4">
        <h1 className="text-xl font-bold">Pluggy</h1>
        <p className="rounded border border-amber-900 bg-amber-950/40 px-3 py-2 text-sm text-amber-300">
          Integração não configurada. Adicione <code>PLUGGY_API_KEY</code> (ou{' '}
          <code>PLUGGY_CLIENT_ID</code> + <code>PLUGGY_CLIENT_SECRET</code>) e{' '}
          <code>PLUGGY_ACCOUNTS</code> (lista de contas do dashboard) ao <code>.env</code> e
          reinicie o backend.
        </p>
      </div>
    )
  }

  const byBank = new Map<string, PluggyAccount[]>()
  for (const a of accounts ?? []) {
    const k = a.bank ?? 'outros'
    byBank.set(k, [...(byBank.get(k) ?? []), a])
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h1 className="text-xl font-bold">Pluggy</h1>
          <p className="text-xs text-zinc-500">
            Contas importadas diretamente da API — sem upload de documentos.
            {status.auth === 'api_key' ? ' (chave de API)' : ' (client credentials)'}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="date"
            value={fromDate}
            onChange={(e) => setFromDate(e.target.value)}
            title="Sincronizar a partir desta data (vazio = incremental)"
            className="rounded-md border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300"
          />
          <input
            type="date"
            value={toDate}
            onChange={(e) => setToDate(e.target.value)}
            title="Sincronizar até esta data"
            className="rounded-md border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300"
          />
          <button
            disabled={syncMutation.isPending}
            onClick={() => syncMutation.mutate()}
            className="rounded-md bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-900 hover:bg-white disabled:opacity-40"
            title={fromDate || toDate ? 'Ressincronizar o período indicado' : 'Importar transações novas'}
          >
            {syncMutation.isPending
              ? 'Sincronizando…'
              : fromDate || toDate
                ? 'Ressincronizar período'
                : 'Sincronizar contas'}
          </button>
        </div>
      </div>

      {error && (
        <p className="rounded border border-red-900 bg-red-950/40 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      )}
      {message && (
        <p className="rounded border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-300">
          {message}
        </p>
      )}

      <div className="flex flex-wrap gap-3 text-xs text-zinc-500">
        <span>{status.accounts} conta(s) configurada(s)</span>
        <span>{status.items} item(ns) importado(s) via Pluggy</span>
        <span>1.189 transações já mapeadas no teste</span>
      </div>

      {accounts && accounts.length === 0 ? (
        <p className="text-sm text-zinc-600">
          Nenhuma conta configurada — preencha <code>PLUGGY_ACCOUNTS</code> no <code>.env</code>.
        </p>
      ) : (
        <div className="space-y-3">
          {[...byBank.entries()].map(([bank, list]) => (
            <section key={bank} className="overflow-hidden rounded border border-zinc-800 bg-zinc-900">
              <div className="flex items-center gap-2 bg-zinc-900 px-3 py-2 text-sm font-semibold text-zinc-300">
                {bankLabel(bank)}
              </div>
              <ul className="divide-y divide-zinc-800 border-t border-zinc-800">
                {list.map((a) => (
                  <AccountRow key={a.pluggy_account_id} a={a} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}
