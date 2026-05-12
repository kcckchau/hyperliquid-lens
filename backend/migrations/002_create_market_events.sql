-- Enums for the market event system

CREATE TYPE event_type AS ENUM (
    'liquidity_sweep',
    'liquidation_cascade'
);

CREATE TYPE event_lifecycle AS ENUM (
    'detected',
    'confirming',
    'confirmed',
    'reclassified',
    'expired'
);

-- Outcomes are empirically observed, not assumed.
-- ExpectationFailed tracks when the statistically expected followup did not materialise.
-- ReclaimAnomaly: price reclaims a key level in a way inconsistent with the event context.
CREATE TYPE outcome_kind AS ENUM (
    'pending',
    'reversal_followed',
    'continuation_followed',
    'exhaustion_detected',
    'absorption_detected',
    'expectation_failed',
    'reclaim_anomaly'
);

CREATE TYPE sweep_direction AS ENUM (
    'bullish',
    'bearish'
);

CREATE TYPE cascade_direction AS ENUM (
    'long_liq',
    'short_liq'
);

-- Regime is a context tag for future filtering. Detection of regime is not implemented here.
CREATE TYPE market_regime AS ENUM (
    'trend',
    'range',
    'volatility_expansion',
    'volatility_compression',
    'momentum_acceleration',
    'chop',
    'unknown'
);

-- -----------------------------------------------------------------------
-- market_events
-- Central table for all detected structural market events.
-- Sweep and cascade fields are nullable and mutually exclusive by event_type.
-- The outcome columns are filled in after the observation window closes.
-- -----------------------------------------------------------------------
CREATE TABLE market_events (
    id                      BIGSERIAL PRIMARY KEY,
    coin                    TEXT NOT NULL,
    interval                TEXT NOT NULL,              -- '1m' | '5m' | '15m' | '1h' | '4h' | '1d'
    event_type              event_type NOT NULL,
    lifecycle               event_lifecycle NOT NULL DEFAULT 'detected',

    -- Sweep-specific (NULL for cascade)
    sweep_direction         sweep_direction,
    level_price             NUMERIC,                    -- the swing level that was pierced
    sweep_extreme           NUMERIC,                    -- wick tip (high for bearish, low for bullish)
    wick_pct                NUMERIC,                    -- fractional distance past level
    close_price             NUMERIC,                    -- candle close (must be back inside level)

    -- Cascade-specific (NULL for sweep)
    cascade_direction       cascade_direction,
    cascade_start_price     NUMERIC,
    liq_count_total         INT,
    candles_sustained       INT,
    volume_acceleration     NUMERIC,                    -- vol[last] / vol[first] in the cascade window

    -- Shared fields
    event_ts_ms             BIGINT NOT NULL,            -- candle open timestamp of the triggering candle
    candle_volume           NUMERIC NOT NULL,

    -- HTF confluence: JSON array of {interval, level_price, age_candles}
    htf_confluence          JSONB NOT NULL DEFAULT '[]',

    -- Outcome (written by outcome_tracker after observation window)
    outcome                 outcome_kind NOT NULL DEFAULT 'pending',
    -- JSON: {magnitude_pct, duration_ms, max_extension, failure_note}
    outcome_detail          JSONB,
    outcome_resolved_ts     BIGINT,

    -- Regime tag (written by regime classifier when it exists)
    regime                  market_regime,

    -- If this event was reclassified from another (e.g. sweep -> cascade)
    reclassified_from       BIGINT REFERENCES market_events(id),

    inserted_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_events_coin_ts       ON market_events (coin, event_ts_ms DESC);
CREATE INDEX idx_events_type          ON market_events (event_type);
CREATE INDEX idx_events_lifecycle     ON market_events (lifecycle);
CREATE INDEX idx_events_interval      ON market_events (interval);
CREATE INDEX idx_events_outcome       ON market_events (outcome);
CREATE INDEX idx_events_coin_interval ON market_events (coin, interval, event_ts_ms DESC);
