-- Tracks the last known trade timestamp per coin.
-- Written every 30 s by the heartbeat background task.
-- Used by /health to detect stale feeds and by startup gap detection (P0C).
CREATE TABLE IF NOT EXISTS system_heartbeats (
    coin              TEXT PRIMARY KEY,
    last_trade_ts_ms  BIGINT NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
