import { useMemo, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { pluggyApi } from '../api/client'
import type { PluggyConnector, PluggyItem, PluggySyncResult } from '../lib/types'

const STATUS_LABELS: Record<string, string> = {
  CREATED: 'Criada',
  UPDATING: 'Sincronizando…',
  LOGIN_DONE: 'Login concluído',
  UPDATED: 'Atualizada',
  WAITING_USER_INPUT: 'Aguardando login',
  WAITING_USER_ACTION: 'Aguardando ação',
  ERROR: 'Erro',
  PARTIAL_SUCCESS: 'Sucesso parcial',
}

function statusStyle(s: string): string {
  if (s === 'UPDATED' || s === 'LOGIN_DONE') return 'text-emerald-400'
  if (s === 'ERROR') return 'text-red-400'
  if (s === 'UPDATING' || s === 'WAITING_USER_INPUT' || s === 'WAITING_USER_ACTION')
    return 'text-amber-400'
  return 'text-zinc-400'
}

function fmtBRL(v: number | null | undefined, signed = false): string {
  if (v == null) return '—'
  const abs = Math.abs(v)
  const s = abs.toLocaleString('pt-BR', {
    style: 'currency',
    currency: 'BRL',
    minimumFractionDigits: 2,
  })
  if (signed && v < 0) return `-${s}`
  return v < 0 ? `-${s}` : s
}

function fmtWhen(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return d.toLocaleDateString('pt-BR') + ' ' + d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
}

const ACCOUNT_TYPE_LABELS: Record<string, string> = {
  BANK: 'Conta',
  CREDIT: 'Cartão',
  INVESTMENT: 'Investimento',
  LOAN: 'Empréstimo',
  BROKERAGE: 'Corretora',
}

const SUBTYPE_LABELS: Record<string, string> = {
  CHECKING_ACCOUNT: 'Conta corrente',
  SAVINGS_ACCOUNT: 'Poupança',
  CREDIT_CARD: 'Cartão de crédito',
  MONEY_MARKET: 'Mercado monetário',
}

function accountLabel(a: { account_type: string | null; subtype: string | null }): string {
  const t = a.account_type ? (ACCOUNT_TYPE_LABELS[a.account_type] ?? a.account_type) : ''
  const s = a.subtype ? (SUBTYPE_LABELS[a.subtype] ?? a.subtype) : ''
  return [t, s].filter(Boolean).join(' · ')
}

function ConnectorRow({
  c,
  busy,
  onConnect,
}: {
  c: PluggyConnector
  busy: boolean
  onConnect: (c: PluggyConnector, params: Record<string, string>) => void
}) {
  const [open, setOpen] = useState(false)
  const [params, setParams] = useState<Record<string, string>>({})
  const creds = c.credentials ?? []

  return (
    <li className="rounded border border-zinc-800 bg-zinc-900">
      <div className="flex items-center gap-3 px-3 py-2">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{c.name}</p>
          <p className="text-xs text-zinc-500">
            {c.kind === 'BUSINESS_BANK' ? 'Empresas' : 'Pessoa física'}
            {c.open_finance ? ' · Open Finance' : ''}
            {c.oauth ? ' · OAuth' : c.mfa ? ' · MFA' : ''}
          </p>
        </div>
        {!c.oauth && creds.length > 0 && (
          <button
            onClick={() => setOpen((o) => !o)}
            className="rounded-md border border-zinc-700 px-2 py-1 text-xs text-zinc-400 hover:text-zinc-100"
          >
            {open ? 'Ocultar credenciais' : 'Credenciais'}
          </button>
        )}
        <button
          disabled={busy}
          onClick={() => onConnect(c, params)}
          className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-white disabled:opacity-40"
        >
          Conectar
        </button>
      </div>
      {open && !c.oauth && (
        <div className="space-y-2 border-t border-zinc-800 px-3 py-3">
          {creds.map((cr) => (
            <label key={cr.name} className="block text-sm">
              <span className="text-xs text-zinc-500">
                {cr.label ?? cr.name}
                {cr.optional ? ' (opcional)' : ''}
              </span>
              <input
                type={cr.type === 'password' ? 'password' : 'text'}
                value={params[cr.name] ?? ''}
                onChange={(e) => setParams((p) => ({ ...p, [cr.name]: e.target.value }))}
                placeholder={cr.placeholder ?? ''}
                className="mt-0.5 w-full rounded-md border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-sm"
              />
            </label>
          ))}
          <p className="text-[11px] text-zinc-600">
            Sandbox: usuário <code className="text-zinc-500">user-ok</code>, senha{' '}
            <code className="text-zinc-500">password-ok</code>, MFA{' '}
            <code className="text-zinc-500">123456</code>
          </p>
        </div>
      )}
    </li>
  )
}

function ItemCard({
  item,
  onDone,
  onDelete,
}: {
  item: PluggyItem
  onDone: () => void
  onDelete: (item: PluggyItem) => void
}) {
  const [busy, setBusy] = useState<string | null>(null)
  const [result, setResult] = useState<PluggySyncResult | null>(null)

  const run = async (action: 'sync' | 'import') => {
    setBusy(action)
    setResult(null)
    try {
      const r = action === 'sync' ? await pluggyApi.sync(item.id) : await pluggyApi.import(item.id)
      setResult(r)
      onDone()
    } finally {
      setBusy(null)
    }
  }

  return (
    <li className="overflow-hidden rounded border border-zinc-800 bg-zinc-900">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{item.connector_name ?? 'Pluggy'}</p>
          <p className="text-xs text-zinc-500">
            {item.accounts.length} conta(s) · {item.item_count} itens · última sync{' '}
            {fmtWhen(item.last_sync_at ?? item.last_updated_at)}
          </p>
        </div>
        <span className={`text-xs font-medium ${statusStyle(item.status)}`}>
          {STATUS_LABELS[item.status] ?? item.status}
        </span>
        <div className="flex items-center gap-1.5">
          <button
            disabled={busy != null || item.status === 'WAITING_USER_INPUT'}
            onClick={() => run('sync')}
            className="rounded-md border border-zinc-700 px-2 py-1 text-xs hover:border-zinc-500 disabled:opacity-40"
          >
            {busy === 'sync' ? 'Sincronizando…' : 'Sincronizar'}
          </button>
          <button
            disabled={busy != null}
            onClick={() => run('import')}
            className="rounded-md border border-zinc-700 px-2 py-1 text-xs hover:border-zinc-500 disabled:opacity-40"
          >
            {busy === 'import' ? 'Importando…' : 'Importar'}
          </button>
          <button
            disabled={busy != null}
            onClick={() => onDelete(item)}
            className="rounded-md border border-red-900 px-2 py-1 text-xs text-red-400 hover:border-red-700 disabled:opacity-40"
          >
            Excluir
          </button>
        </div>
      </div>

      {result && (
        <div className="border-t border-zinc-800 px-3 py-2 text-xs text-zinc-400">
          {result.imported > 0
            ? `${result.imported} novo(s) item(ns) importado(s).`
            : result.pending
              ? 'Ainda sincronizando — aguarde e clique em “Importar”.'
              : result.status === 'ERROR'
                ? `Erro na sincronização${result.status_detail ? `: ${result.status_detail}` : '.'}`
                : 'Sem itens novos (já importados).'}
        </div>
      )}

      {item.accounts.length > 0 && (
        <ul className="divide-y divide-zinc-800 border-t border-zinc-800">
          {item.accounts.map((a) => (
            <li key={a.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-zinc-950 px-3 py-2 text-sm">
              <span className="min-w-0 flex-1 truncate text-zinc-300">{a.name}</span>
              <span className="hidden text-xs text-zinc-600 sm:inline">{accountLabel(a)}</span>
              {a.account_type === 'CREDIT' ? (
                <span className="text-xs text-zinc-500">
                  limite {fmtBRL(a.credit_limit)}
                  {a.due_date && <> · vence {a.due_date.slice(8, 10)}/{a.due_date.slice(5, 7)}</>}
                </span>
              ) : (
                <span className="text-sm font-medium tabular-nums text-zinc-100">
                  {fmtBRL(a.balance)}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </li>
  )
}

export default function Pluggy() {
  const qc = useQueryClient()
  const [query, setQuery] = useState('')
  const [connecting, setConnecting] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const { data: status } = useQuery({ queryKey: ['pluggy-status'], queryFn: pluggyApi.status })
  const { data: connectors, isFetching: fetchingConnectors } = useQuery({
    queryKey: ['pluggy-connectors'],
    queryFn: pluggyApi.connectors,
    enabled: status?.configured,
  })
  const { data: items, refetch: refetchItems } = useQuery({
    queryKey: ['pluggy-items'],
    queryFn: pluggyApi.list,
    enabled: status?.configured,
  })

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['pluggy-items'] })
    refetchItems()
  }

  const filtered = useMemo(() => {
    if (!connectors) return []
    const q = query.trim().toLowerCase()
    const list = q
      ? connectors.filter(
          (c) => c.name.toLowerCase().includes(q) || (c.kind ?? '').toLowerCase().includes(q),
        )
      : connectors
    return list
      .filter((c) => c.kind === 'PERSONAL_BANK' || c.kind === 'BUSINESS_BANK')
      .slice(0, 60)
  }, [connectors, query])

  const connect = async (c: PluggyConnector, params: Record<string, string>) => {
    setConnecting(c.id.toString())
    setError(null)
    try {
      const item = await pluggyApi.create({
        connector_id: c.id,
        parameters: Object.keys(params).length ? params : undefined,
      })
      invalidate()

      if (item.oauth_url) {
        // OAuth: open Pluggy's authorize page in a new tab; it redirects back
        // to Pluggy's own callback, then we poll until the item syncs.
        window.open(item.oauth_url, '_blank', 'noopener')
        for (let i = 0; i < 40; i++) {
          await new Promise((r) => setTimeout(r, 3000))
          const fresh = await pluggyApi.refresh(item.id)
          invalidate()
          if (fresh.status === 'UPDATED' || fresh.status === 'LOGIN_DONE') {
            const res = await pluggyApi.sync(item.id)
            invalidate()
            if (res.imported > 0) {
              setError(`${res.imported} item(ns) importado(s) do ${c.name}.`)
            } else if (res.pending) {
              setError('Login feito — clique em “Importar” quando a sincronização terminar.')
            }
            return
          }
          if (fresh.status === 'ERROR') {
            setError(`Falha ao conectar ${c.name} no banco.`)
            return
          }
        }
        setError('Login não concluído — verifique a janela aberta e clique em “Sincronizar”.')
      } else {
        // Direct credentials: trigger sync right away.
        const res = await pluggyApi.sync(item.id)
        invalidate()
        if (res.imported > 0) {
          setError(`${res.imported} item(ns) importado(s) do ${c.name}.`)
        } else if (res.pending) {
          setError('Sincronizando… clique em “Importar” quando terminar.')
        }
      }
    } catch (e) {
      setError(`Falha ao conectar: ${(e as { message?: string }).message ?? 'erro desconhecido'}`)
    } finally {
      setConnecting(null)
    }
  }

  const syncAll = async () => {
    const r = await pluggyApi.syncAll()
    invalidate()
    setError(`Sincronização em massa concluída: ${r.imported} item(ns) novos.`)
  }

  const remove = async (item: PluggyItem) => {
    if (!window.confirm(`Desconectar ${item.connector_name ?? 'este banco'}?`)) return
    await pluggyApi.remove(item.id)
    invalidate()
  }

  if (!status?.configured) {
    return (
      <div className="space-y-4">
        <h1 className="text-xl font-bold">Pluggy</h1>
        <p className="rounded border border-amber-900 bg-amber-950/40 px-3 py-2 text-sm text-amber-300">
          Integração não configurada. Adicione <code>PLUGGY_API_KEY</code> (recomendado para um
          usuário só) — ou <code>PLUGGY_CLIENT_ID</code> + <code>PLUGGY_CLIENT_SECRET</code> — ao{' '}
          <code>.env</code> e reinicie o backend.
        </p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-xl font-bold">Pluggy</h1>
        <div className="flex items-center gap-3 text-xs text-zinc-500">
          <span>{status.auth === 'api_key' ? 'chave de API' : status.auth === 'client' ? 'client credentials' : ''}</span>
          <span>{status.items} conexão(ões)</span>
          <span>{status.accounts} conta(s)</span>
          {items && items.length > 0 && (
            <button
              onClick={syncAll}
              className="rounded-md border border-zinc-700 px-2.5 py-1 text-xs hover:border-zinc-500"
            >
              Sincronizar todas
            </button>
          )}
        </div>
      </div>

      {error && (
        <p className="rounded border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-300">
          {error}
        </p>
      )}

      <section className="space-y-2">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-semibold text-zinc-400">Conectar conta</h2>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar banco…"
            className="ml-auto w-52 rounded-md border border-zinc-700 bg-zinc-900 px-2 py-1.5 text-sm placeholder:text-zinc-600"
          />
        </div>
        {fetchingConnectors && !connectors ? (
          <p className="text-sm text-zinc-500">Carregando bancos…</p>
        ) : (
          <ul className="grid gap-2 sm:grid-cols-2">
            {filtered.map((c) => (
              <ConnectorRow key={c.id} c={c} busy={connecting != null} onConnect={connect} />
            ))}
          </ul>
        )}
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-semibold text-zinc-400">Conectadas</h2>
        {!items || items.length === 0 ? (
          <p className="text-sm text-zinc-600">
            Nenhuma conexão ainda. Escolha um banco acima (sandbox: “MeuPluggy”, usuário{' '}
            <code>ralph.bragg@gmail.com</code> / <code>P@ssword01</code>).
          </p>
        ) : (
          <ul className="space-y-2">
            {items.map((i) => (
              <ItemCard key={i.id} item={i} onDone={invalidate} onDelete={remove} />
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}
