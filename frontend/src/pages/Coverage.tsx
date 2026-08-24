import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { coverageApi, sourcesApi } from '../api/client'

function monthLabel(ym: string): string {
  const [y, m] = ym.split('-')
  return `${m}/${y.slice(2)}`
}

export default function Coverage() {
  const qc = useQueryClient()
  const { data } = useQuery({ queryKey: ['coverage'], queryFn: coverageApi.get })

  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      sourcesApi.update(id, { enabled }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['coverage'] }),
  })

  const months = data?.months ?? []
  const sources = data?.sources ?? []

  return (
    <div>
      <h1 className="mb-2 text-xl font-bold">Cobertura de fontes</h1>
      <p className="mb-4 text-sm text-zinc-500">
        Fontes fundamentais (bancos × conta/cartão) e os últimos {months.length} meses.{' '}
        <span className="text-emerald-400">●</span> presente ·{' '}
        <span className="text-zinc-700">●</span> ausente.
      </p>

      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr>
              <th className="py-2 pr-4 text-left font-medium text-zinc-400">Fonte</th>
              {months.map((m) => (
                <th
                  key={m}
                  className="px-1 py-2 text-center text-[10px] font-normal text-zinc-500"
                >
                  {monthLabel(m)}
                </th>
              ))}
              <th className="py-2 pl-4 text-left font-medium text-zinc-400">Último envio</th>
            </tr>
          </thead>
          <tbody>
            {sources.map((s) => {
              const present = new Set(s.present)
              return (
                <tr key={s.id} className={s.enabled ? '' : 'opacity-40'}>
                  <td className="py-2 pr-4">
                    <span className="font-medium">{s.name}</span>
                    <button
                      onClick={() => toggle.mutate({ id: s.id, enabled: !s.enabled })}
                      className="ml-2 text-xs text-zinc-500 hover:text-zinc-200"
                    >
                      {s.enabled ? 'desativar' : 'ativar'}
                    </button>
                  </td>
                  {months.map((m) => (
                    <td key={m} className="px-1 py-2 text-center">
                      <span className={present.has(m) ? 'text-emerald-400' : 'text-zinc-700'}>
                        ●
                      </span>
                    </td>
                  ))}
                  <td className="py-2 pl-4 text-xs text-zinc-500">
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
