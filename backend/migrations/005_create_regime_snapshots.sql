-- Stores the current regime state per (coin, timeframe).
-- One row per (coin, interval_label). Upserted every 15 minutes by the
-- regime engine. Hysteresis state is persisted so it survives restarts.
CREATE TABLE IF NOT EXISTS regime_snapshots (
    coin              TEXT NOT NULL,
    interval_label    TEXT NOT NULL,   -- '1h' or '4h'

    -- Current confirmed regime
    regime            TEXT NOT NULL,   -- TREND_UP | TREND_DOWN | RANGE | HIGH_VOL_CHOP | LOW_VOL_COMPRESSION
    confidence        NUMERIC(6,4)  NOT NULL DEFAULT 0,
    candles_in_regime INT           NOT NULL DEFAULT 1,

    -- Challenger regime accumulating before it can flip (hysteresis)
    pending_regime    TEXT,
    pending_count     INT           NOT NULL DEFAULT 0,

    -- Change history
    previous_regime   TEXT,
    changed_at        TIMESTAMPTZ,

    -- Metadata
    computed_at       TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    candle_ts_ms      BIGINT        NOT NULL DEFAULT 0,

    PRIMARY KEY (coin, interval_label)
);
