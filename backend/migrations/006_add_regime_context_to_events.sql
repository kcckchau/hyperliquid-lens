-- Attaches regime context (1h + 4h snapshot) to every detected event.
-- Populated at detection time by querying the latest regime_snapshots row.
-- NULL for events detected before the regime engine has produced any data.
ALTER TABLE market_events
    ADD COLUMN IF NOT EXISTS regime_context JSONB;
