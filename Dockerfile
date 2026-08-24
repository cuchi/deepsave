# syntax=docker/dockerfile:1

# ---------- Stage 1: build frontend ----------
FROM node:22-alpine AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---------- Stage 2: build backend ----------
FROM rust:1.97-slim AS backend-builder
WORKDIR /app/backend

# aws-lc-rs (reqwest's rustls backend) needs cmake on slim images.
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies by building with a stub main first.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release

# Build the real binary (migrations are embedded at compile time).
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN touch src/main.rs && cargo build --release

# ---------- Stage 3: runtime ----------
FROM debian:trixie-slim AS runtime

# Tesseract OCR + Portuguese language data (used for receipt photos / scans).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tesseract-ocr \
    tesseract-ocr-por \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=backend-builder /app/backend/target/release/deepsave-backend /app/deepsave-backend
COPY --from=backend-builder /app/backend/migrations /app/migrations
COPY --from=frontend-builder /app/frontend/dist /app/dist

ENV STATIC_DIR=/app/dist \
    PORT=8080

EXPOSE 8080
CMD ["/app/deepsave-backend"]
