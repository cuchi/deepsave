# DeepSave

AI-augmented personal finance manager. Single-user, self-hosted.

- **Backend**: Rust (Axum) + PostgreSQL (SQLx)
- **Frontend**: React (Vite + TypeScript + Tailwind)
- **AI**: DeepSeek (extraction, categorization, linking)

See [`PLAN.md`](./PLAN.md) for the full design.

## Quick start (dev)

```bash
# 1. start postgres
docker compose up -d postgres

# 2. backend (runs migrations on startup)
cp .env.example .env   # adjust values
make dev-backend       # http://localhost:8080/api/health

# 3. frontend
cd frontend && npm install && npm run dev   # http://localhost:5173
```

The Vite dev server proxies `/api` to the backend on port 8080.

## Apply migrations manually (optional)

The backend applies migrations automatically at startup. To run them manually:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
make migrate
```
