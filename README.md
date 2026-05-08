# Hyperliquid Lens

A production-grade real-time trade indexer and live dashboard for the [Hyperliquid](https://hyperliquid.xyz) DEX.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          HYPERLIQUID LENS                                   │
│                                                                             │
│  ┌──────────────────────┐         ┌──────────────────────────────────────┐  │
│  │  Hyperliquid WSS     │         │           Next.js Frontend           │  │
│  │  wss://api.hyper...  │         │                                      │  │
│  │  • BTC trades        │         │  ┌─────────────┐  ┌───────────────┐ │  │
│  │  • ETH trades        │         │  │CandleChart  │  │  TradeFeed   │ │  │
│  │  • SOL trades        │         │  │(lightweight │  │  (live WS)   │ │  │
│  └──────────┬───────────┘         │  │ -charts)    │  │              │ │  │
│             │ wss                 │  └──────┬──────┘  └──────┬───────┘ │  │
│             ▼                     │         │ REST            │ WS      │  │
│  ┌──────────────────────┐         │         └────────┬────────┘         │  │
│  │   Rust Backend       │         └──────────────────┼──────────────────┘  │
│  │  (Axum + Tokio)      │                            │                     │
│  │                      │◄───────────────────────────┘                     │
│  │  ┌────────────────┐  │   REST  GET /trades                              │
│  │  │  ws_client.rs  │  │         GET /trades/summary                      │
│  │  │  (ingester)    │  │   WS    GET /ws/trades?coin=ETH                  │
│  │  └───────┬────────┘  │                                                  │
│  │          │ INSERT     │                                                  │
│  │  ┌───────▼────────┐  │                                                  │
│  │  │  trades.rs     │  │                                                  │
│  │  │  (sqlx repo)   │  │                                                  │
│  │  └───────┬────────┘  │                                                  │
│  │          │            │                                                  │
│  │  ┌───────▼────────┐  │                                                  │
│  │  │  broadcast::   │  │                                                  │
│  │  │  Sender<Trade> │  │                                                  │
│  │  └───────┬────────┘  │                                                  │
│  │          │ fan-out    │                                                  │
│  │  ┌───────▼────────┐  │                                                  │
│  │  │  routes.rs WS  │──┼──► WebSocket clients                            │
│  │  │  handlers      │  │                                                  │
│  │  └────────────────┘  │                                                  │
│  └──────────────────────┘                                                  │
│             │                                                               │
│             ▼                                                               │
│  ┌──────────────────────┐                                                   │
│  │  PostgreSQL 16        │                                                  │
│  │  Table: trades        │                                                  │
│  │  Index: (coin, ts)    │                                                  │
│  └──────────────────────┘                                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Features

- **Real-time WebSocket ingestion** — subscribes to Hyperliquid's live trade feed for BTC, ETH, SOL (configurable)
- **Persistent storage** — every trade is deduplicated and written to PostgreSQL via `sqlx`
- **Exponential backoff reconnection** — ingester automatically recovers from network drops
- **Live broadcast** — `tokio::sync::broadcast` fans out incoming trades to all connected WebSocket clients with zero extra DB queries
- **REST API** — paginated trade history + OHLCV aggregation queries
- **Live dashboard** — TradingView-quality candlestick chart (lightweight-charts) + scrolling live trade feed
- **Dark terminal aesthetic** — monospace fonts, green/red color coding, minimal chrome
- **Docker Compose** — one command to start the full stack with health checks

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Ingestion | Rust · Tokio · tokio-tungstenite |
| API server | Axum (REST + WebSocket) |
| Database | PostgreSQL 16 · sqlx |
| Frontend | Next.js 14 (App Router) · TypeScript |
| Charts | lightweight-charts (TradingView) |
| Styling | Tailwind CSS |
| Container | Docker Compose |

## Quick Start

### Option A — Full Docker stack

```bash
# Clone and start everything
git clone https://github.com/your-handle/hyperliquid-lens
cd hyperliquid-lens
make up

# Open browser
open http://localhost:3000
```

### Option B — Local development

**Prerequisites:** Rust (rustup), Node.js 20+, Docker Desktop

```bash
# 1. Start PostgreSQL only
make dev-db

# 2. Configure backend environment
cp backend/.env.example backend/.env
# Edit backend/.env if needed

# 3. Start backend (new terminal)
make dev-backend

# 4. Install frontend deps and start (new terminal)
make frontend-install
make dev-frontend

# Open browser
open http://localhost:3000
```

## API Reference

### `GET /health`
Returns `{"status": "ok"}`.

### `GET /trades`

| Parameter | Type | Description |
|-----------|------|-------------|
| `coin` | string | Required. e.g. `ETH` |
| `from` | integer | Unix milliseconds (inclusive) |
| `to` | integer | Unix milliseconds (inclusive) |
| `limit` | integer | Default 100, max 1000 |

### `GET /trades/summary`

| Parameter | Type | Description |
|-----------|------|-------------|
| `coin` | string | Required |
| `interval` | string | `1m` `5m` `15m` `1h` `4h` `1d` (default `1h`) |
| `from` | integer | Unix milliseconds |
| `to` | integer | Unix milliseconds |

Returns OHLCV candles: `{ coin, interval, candles: [{ bucket_ms, open, high, low, close, volume }] }`

### `WS /ws/trades?coin=ETH`

Streams `Trade` JSON objects as they arrive:

```json
{
  "coin": "ETH",
  "side": "B",
  "price": "3200.50",
  "size": "0.5000",
  "timestamp_ms": 1718000000000,
  "trade_hash": "0xabc...",
  "is_liquidation": false
}
```

## Configuration

### Backend (`backend/.env`)

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `PORT` | `3001` | API listen port |
| `COINS` | `BTC,ETH,SOL` | Comma-separated coins to index |
| `RUST_LOG` | `info` | Log level (`trace` `debug` `info` `warn` `error`) |

### Frontend

| Variable | Default | Description |
|----------|---------|-------------|
| `NEXT_PUBLIC_API_URL` | `http://localhost:3001` | Backend REST base URL |
| `NEXT_PUBLIC_WS_URL` | `ws://localhost:3001` | Backend WebSocket base URL |

## Project Structure

```
hyperliquid-lens/
├── backend/
│   ├── src/
│   │   ├── main.rs               # Entry point, wires all components
│   │   ├── config.rs             # Config from env vars
│   │   ├── ingester/
│   │   │   ├── mod.rs
│   │   │   ├── ws_client.rs      # Hyperliquid WS + reconnect logic
│   │   │   └── parser.rs         # Typed trade structs + deserialisation
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   └── trades.rs         # sqlx repository (insert, fetch, OHLCV)
│   │   └── api/
│   │       ├── mod.rs
│   │       └── routes.rs         # Axum handlers + WS broadcast
│   ├── migrations/
│   │   └── 001_create_trades.sql
│   ├── Cargo.toml
│   └── Dockerfile
├── frontend/
│   ├── app/
│   │   ├── layout.tsx
│   │   └── page.tsx              # Main dashboard
│   ├── components/
│   │   ├── CoinSelector.tsx      # Coin dropdown
│   │   ├── TradeFeed.tsx         # Live scrolling trade list
│   │   ├── CandlestickChart.tsx  # OHLCV chart (lightweight-charts)
│   │   └── StatsBar.tsx          # 24h stats
│   ├── lib/
│   │   ├── ws.ts                 # useLiveTrades hook
│   │   └── api.ts                # REST helpers
│   ├── Dockerfile
│   └── package.json
├── docker-compose.yml
├── Makefile
└── README.md
```

## Development Commands

```bash
make dev-db          # Start PostgreSQL only (Docker)
make dev-backend     # cargo run
make dev-frontend    # npm run dev
make up              # Full stack (Docker Compose)
make down            # Stop all services
make logs            # Tail all container logs
make clean           # Remove containers + volumes
make migrate         # Run pending DB migrations
make backend-check   # fmt + clippy
make backend-test    # cargo test
```

## License

MIT
