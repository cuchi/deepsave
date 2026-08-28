import { FormEvent, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { authApi } from '../api/client'

export default function Login() {
  const [password, setPassword] = useState('')
  const [error, setError] = useState(false)
  const [busy, setBusy] = useState(false)
  const nav = useNavigate()
  const qc = useQueryClient()

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(false)
    try {
      await authApi.login(password)
      qc.setQueryData(['me'], { authenticated: true })
      nav('/')
    } catch {
      setError(true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid min-h-screen place-items-center bg-zinc-950 p-4 text-zinc-100">
      <form
        onSubmit={submit}
        className="w-full max-w-sm space-y-4 rounded-lg border border-zinc-800 bg-zinc-900 p-6"
      >
        <h1 className="text-xl font-bold tracking-tight">DeepSave</h1>
        <p className="text-sm text-zinc-400">Entre com sua senha</p>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Senha"
          autoFocus
          className="field"
        />
        {error && <p className="text-sm text-red-400">Senha incorreta</p>}
        <button
          disabled={busy}
          className="w-full rounded bg-zinc-100 py-2 text-sm font-medium text-zinc-900 hover:bg-white disabled:opacity-50"
        >
          {busy ? 'Entrando…' : 'Entrar'}
        </button>
      </form>
    </div>
  )
}
