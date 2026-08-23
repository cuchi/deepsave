.PHONY: help up down logs migrate dev-backend dev-frontend build-backend build-frontend test

help:
	@printf "DeepSave dev targets:\n"
	@printf "  make up             start postgres (docker compose)\n"
	@printf "  make down           stop postgres\n"
	@printf "  make logs           tail postgres logs\n"
	@printf "  make migrate        apply DB migrations (sqlx-cli)\n"
	@printf "  make dev-backend    run backend (cargo run)\n"
	@printf "  make dev-frontend   run frontend (vite dev)\n"
	@printf "  make build-backend  cargo build --release\n"
	@printf "  make build-frontend npm run build\n"
	@printf "  make test           cargo test\n"

up:
	docker compose up -d postgres

down:
	docker compose down

logs:
	docker compose logs -f postgres

migrate:
	cd backend && sqlx migrate run

dev-backend:
	cd backend && cargo run

dev-frontend:
	cd frontend && npm run dev

build-backend:
	cd backend && cargo build --release

build-frontend:
	cd frontend && npm run build

test:
	cd backend && cargo test
