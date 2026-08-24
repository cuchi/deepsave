import { Navigate, Route, Routes } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { authApi } from './api/client'
import Login from './pages/Login'
import MonthView from './pages/MonthView'
import Categories from './pages/Categories'
import Upload from './pages/Upload'
import Review from './pages/Review'
import Coverage from './pages/Coverage'
import Memory from './pages/Memory'
import Recurring from './pages/Recurring'
import Tags from './pages/Tags'
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
        <Route path="/" element={<MonthView />} />
        <Route path="/months/:ym" element={<MonthView />} />
        <Route path="/categories" element={<Categories />} />
        <Route path="/upload" element={<Upload />} />
        <Route path="/review" element={<Review />} />
        <Route path="/coverage" element={<Coverage />} />
        <Route path="/memory" element={<Memory />} />
        <Route path="/recurring" element={<Recurring />} />
        <Route path="/tags" element={<Tags />} />
      </Route>
      <Route path="*" element={<Navigate to={authed ? '/' : '/login'} replace />} />
    </Routes>
  )
}
