import { useEffect, useState } from 'react'

interface Health {
  status: string
  db: boolean
}

export default function App() {
  const [health, setHealth] = useState<Health | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    fetch('/api/health')
      .then((r) => r.json())
      .then((d: Health) => setHealth(d))
      .catch(() => setError(true))
  }, [])

  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-zinc-950 text-zinc-100">
      <h1 className="text-4xl font-bold tracking-tight">DeepSave</h1>
      <p className="text-zinc-400">AI-augmented personal finance manager</p>
      <code className="rounded-md bg-zinc-900 px-3 py-1 text-sm text-zinc-300">
        {error
          ? 'backend unreachable'
          : health
            ? `backend: ${health.status}, db: ${health.db}`
            : 'checking…'}
      </code>
    </div>
  )
}
