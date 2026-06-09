ALTER TABLE market_events
    ADD COLUMN source TEXT NOT NULL DEFAULT 'live';

CREATE INDEX idx_events_source ON market_events (source);
