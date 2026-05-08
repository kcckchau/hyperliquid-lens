.PHONY: dev-db dev-backend dev-frontend build up down logs clean

# Start only PostgreSQL for local development
dev-db:
	docker compose up postgres -d
	@echo "PostgreSQL running on port 5433 (default)"
	@echo "  Override with: PG_PORT=5432 make dev-db"
	@echo "  URL: postgres://hyperliquid:hyperliquid@localhost:5433/hyperliquid_lens"

# Run backend locally (requires postgres to be running)
dev-backend:
	cd backend && cargo run

# Run frontend locally
dev-frontend:
	cd frontend && npm run dev

# Build all Docker images
build:
	docker compose build

# Start full stack
up:
	docker compose up -d

# Stop full stack
down:
	docker compose down

# Tail logs
logs:
	docker compose logs -f

# Nuke everything including volumes
clean:
	docker compose down -v --remove-orphans

# Run DB migrations manually (requires DATABASE_URL in env)
migrate:
	cd backend && cargo sqlx migrate run

# Install frontend dependencies
frontend-install:
	cd frontend && npm install

# Format + lint backend
backend-check:
	cd backend && cargo fmt --check && cargo clippy -- -D warnings

# Run backend tests
backend-test:
	cd backend && cargo test
