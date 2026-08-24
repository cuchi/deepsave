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
              Início
            </Link>
            <Link to="/upload" className="hover:text-zinc-100">
              Upload
            </Link>
            <Link to="/review" className="hover:text-zinc-100">
              Revisar
            </Link>
            <Link to="/coverage" className="hover:text-zinc-100">
              Cobertura
            </Link>
            <Link to="/memory" className="hover:text-zinc-100">
              Memória
            </Link>
            <Link to="/categories" className="hover:text-zinc-100">
              Categorias
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
