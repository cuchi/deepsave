import { useState } from 'react'
import { Link, Outlet, useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { authApi } from '../api/client'

const NAV_LINKS = [
  { to: '/', label: 'Gráficos' },
  { to: '/lista', label: 'Lista' },
  { to: '/forecast', label: 'Previsão' },
  { to: '/upload', label: 'Documentos' },
  { to: '/review', label: 'Revisar' },
  { to: '/memory', label: 'Memória' },
  { to: '/recurring', label: 'Recorrentes' },
  { to: '/pluggy', label: 'Pluggy' },
  { to: '/system', label: 'Sistema' },
]

export default function Layout() {
  const qc = useQueryClient()
  const nav = useNavigate()
  const [menuOpen, setMenuOpen] = useState(false)

  const logout = async () => {
    await authApi.logout()
    qc.setQueryData(['me'], { authenticated: false })
    nav('/login')
  }

  const closeMenu = () => setMenuOpen(false)

  return (
    <div className="min-h-screen bg-zinc-950 text-zinc-100">
      <header className="sticky top-0 z-20 border-b border-zinc-800 bg-zinc-950/90 backdrop-blur">
        <div className="flex items-center justify-between gap-2 px-4 py-3 sm:px-6">
          <div className="flex min-w-0 items-center gap-4">
            <Link
              to="/"
              onClick={closeMenu}
              className="shrink-0 text-lg font-bold tracking-tight"
            >
              DeepSave
            </Link>
            {/* Desktop nav */}
            <nav className="hidden items-center gap-4 text-sm text-zinc-400 lg:flex">
              {NAV_LINKS.map((l) => (
                <Link key={l.to} to={l.to} className="whitespace-nowrap hover:text-zinc-100">
                  {l.label}
                </Link>
              ))}
            </nav>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button
              onClick={logout}
              className="rounded-md px-2 py-2 text-sm text-zinc-400 hover:text-zinc-100"
            >
              Sair
            </button>
            {/* Hamburger (mobile/tablet) */}
            <button
              onClick={() => setMenuOpen((o) => !o)}
              aria-label={menuOpen ? 'Fechar menu' : 'Abrir menu'}
              aria-expanded={menuOpen}
              className="grid h-9 w-9 place-items-center rounded-md text-xl leading-none text-zinc-300 hover:bg-zinc-800 lg:hidden"
            >
              {menuOpen ? '✕' : '☰'}
            </button>
          </div>
        </div>
        {/* Mobile nav */}
        {menuOpen && (
          <nav className="border-t border-zinc-800 px-2 py-2 lg:hidden">
            {NAV_LINKS.map((l) => (
              <Link
                key={l.to}
                to={l.to}
                onClick={closeMenu}
                className="block rounded-md px-3 py-2.5 text-sm text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100"
              >
                {l.label}
              </Link>
            ))}
          </nav>
        )}
      </header>
      <main className="mx-auto max-w-4xl px-4 py-6 sm:px-6 sm:py-8">
        <Outlet />
      </main>
    </div>
  )
}
