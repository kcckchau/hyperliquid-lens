CREATE TABLE IF NOT EXISTS trades (
    id             BIGSERIAL PRIMARY KEY,
    coin           TEXT        NOT NULL,
    side           TEXT        NOT NULL CHECK (side IN ('B', 'S')),
    price          NUMERIC     NOT NULL,
    size           NUMERIC     NOT NULL,
    timestamp_ms   BIGINT      NOT NULL,
    trade_hash     TEXT        NOT NULL,
    is_liquidation BOOLEAN     NOT NULL DEFAULT FALSE,
    inserted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast range queries per coin
CREATE INDEX IF NOT EXISTS idx_trades_coin_ts ON trades (coin, timestamp_ms DESC);

-- Deduplicate incoming trades
CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_hash ON trades (trade_hash);
