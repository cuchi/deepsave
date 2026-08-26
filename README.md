# DeepSave

AI-augmented personal finance manager. Single-user, self-hosted.

- **Backend**: Rust (Axum) + PostgreSQL (SQLx)
- **Frontend**: React (Vite + TypeScript + Tailwind)
- **AI**: DeepSeek (extraction, categorization, linking)

See [`PLAN.md`](./PLAN.md) for the full design.

## Run with Docker (recommended)

Builds the backend + frontend into a single image and starts it with Postgres:

```bash
docker compose up --build          # full app at :8080

docker compose watch               # dev: rebuilds & restarts the app on code changes
```

The app is served at http://localhost:8080 (backend serves the built frontend as a SPA).

For an edit-and-reload loop use [`docker compose watch`](https://docs.docker.com/compose/how-tos/file-watch/):
it watches the sources and rebuilds/recreates the `app` container on every save
(BuildKit reuses the cached Rust/node dependency layers, so only the changed
stage recompiles).

## Development

```bash
# 1. start postgres
docker compose up -d postgres

# 2. backend (runs migrations on startup)
cp .env.example .env   # adjust values
cd backend && cargo run       # http://localhost:8080/api/health

# 3. frontend (separate terminal)
cd frontend && npm install && npm run dev   # http://localhost:5173
```

The Vite dev server proxies `/api` to the backend on port 8080.

## Seed fake data

Populates the database with fake categories, accounts, a 3-month spending tree,
transfers, income and recurring rules (useful while testing the UI):

```bash
./scripts/seed.sh
```

Runs from the host via `docker exec` into the Postgres container (override the
container name with `DEEPSEED_PG_CONTAINER` if needed). Re-running it resets and
re-seeds the data tables.

## Backup & restore

Postgres runs in the `deepsave-postgres` container (user/db `deepsave`); uploaded
statements/receipts live in the `appdata` volume (`/app/storage`), **not** in the
database.

Full backup (DB dump + uploaded files) and restore:

```bash
./scripts/backup                 # writes ./backups/deepsave-<timestamp>.dump (+ -appdata.tar.gz)
./scripts/restore                # restores the most recent backup; or pass a timestamp:
./scripts/restore 20250101-120000
```

Both accept `DEEPSAVE_BACKUP_DIR`, `DEEPSAVE_PG_CONTAINER`, `DEEPSAVE_PG_USER`,
`DEEPSAVE_PG_DB`, `DEEPSAVE_APPDATA_VOLUME` env overrides (see headers in the
scripts). Restore **overwrites** the current database and restarts the app.

## Login (dev)

## Login (dev)

If `APP_PASSWORD_HASH`/`APP_PASSWORD` are not set, the default password is `deepsave`.
Set `APP_PASSWORD` (plaintext, hashed at startup) or `APP_PASSWORD_HASH` (argon2) in `.env`
to change it.

## Apply migrations manually (optional)

The backend applies migrations automatically at startup. To run them manually:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
cd backend && sqlx migrate run
```
