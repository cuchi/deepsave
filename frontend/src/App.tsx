import { Navigate, Route, Routes } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { authApi } from './api/client'
import Login from './pages/Login'
import Charts from './pages/Charts'
import Lista from './pages/Lista'
import Upload from './pages/Upload'
import Review from './pages/Review'
import Memory from './pages/Memory'
import Recurring from './pages/Recurring'
import Layout from './components/Layout'

export default function App() {
  const { data, isLoading } = useQuery({
    queryKey: ['me'],
    queryFn: authApi.me,
    retry: false,
  })

  if (isLoading) {
    return (
      <div className="grid min-h-screen place-items-center bg-zinc-950 text-zinc-400">
        carregando…
      </div>
    )
  }

  const authed = data?.authenticated ?? false

  return (
    <Routes>
      <Route
        path="/login"
        element={authed ? <Navigate to="/" replace /> : <Login />}
      />
      <Route element={authed ? <Layout /> : <Navigate to="/login" replace />}>
        <Route path="/" element={<Charts />} />
        <Route path="/lista" element={<Lista />} />
        <Route path="/categories" element={<Navigate to="/memory" replace />} />
        <Route path="/upload" element={<Upload />} />
        <Route path="/review" element={<Review />} />
        <Route path="/coverage" element={<Navigate to="/upload" replace />} />
        <Route path="/memory" element={<Memory />} />
        <Route path="/recurring" element={<Recurring />} />
        <Route path="/tags" element={<Navigate to="/memory" replace />} />
      </Route>
      <Route path="*" element={<Navigate to={authed ? '/' : '/login'} replace />} />
    </Routes>
  )
}
