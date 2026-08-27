import { Link, Outlet, useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { authApi } from '../api/client'

export default function Layout() {
  const qc = useQueryClient()
  const nav = useNavigate()

  const logout = async () => {
    await authApi.logout()
    qc.setQueryData(['me'], { authenticated: false })
    nav('/login')
  }

  return (
    <div className="min-h-screen bg-zinc-950 text-zinc-100">
      <header className="flex items-center justify-between border-b border-zinc-800 px-6 py-3">
        <div className="flex items-center gap-6">
          <Link to="/" className="text-lg font-bold tracking-tight">
            DeepSave
          </Link>
          <nav className="flex gap-4 text-sm text-zinc-400">
            <Link to="/" className="hover:text-zinc-100">
              Gráficos
            </Link>
            <Link to="/lista" className="hover:text-zinc-100">
              Lista
            </Link>
            <Link to="/forecast" className="hover:text-zinc-100">
              Previsão
            </Link>
            <Link to="/upload" className="hover:text-zinc-100">
              Documentos
            </Link>
            <Link to="/review" className="hover:text-zinc-100">
              Revisar
            </Link>
            <Link to="/memory" className="hover:text-zinc-100">
              Memória
            </Link>
            <Link to="/recurring" className="hover:text-zinc-100">
              Recorrentes
            </Link>
            <Link to="/system" className="hover:text-zinc-100">
              Sistema
            </Link>
          </nav>
        </div>
        <button
          onClick={logout}
          className="text-sm text-zinc-400 hover:text-zinc-100"
        >
          Sair
        </button>
      </header>
      <main className="mx-auto max-w-4xl px-6 py-8">
        <Outlet />
      </main>
    </div>
  )
}
