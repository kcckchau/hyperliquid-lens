"use client";

import { startTransition, useCallback, useEffect, useRef, useState } from "react";

export interface Trade {
  coin: string;
  side: "B" | "S";
  price: string;
  size: string;
  timestamp_ms: number;
  trade_hash: string;
  is_liquidation: boolean;
}

export type WsStatus = "connecting" | "connected" | "disconnected" | "error";

const WS_URL =
  process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:3001";

const INITIAL_BACKOFF = 1_000;
const MAX_BACKOFF = 30_000;
const MAX_TRADES = 200;
const MAX_EVENTS = 100;
const FLUSH_INTERVAL_MS = 75;

interface UseLiveTradesOptions {
  coin: string;
  enabled?: boolean;
  maxItems?: number;
}

interface UseLiveTradesResult {
  trades: Trade[];
  status: WsStatus;
  clearTrades: () => void;
}

// ---------------------------------------------------------------------------
// Market event types (mirrors backend detection/types.rs)
// ---------------------------------------------------------------------------

export type EventType = "liquidity_sweep" | "liquidation_cascade";
export type EventLifecycle = "detected" | "confirming" | "confirmed" | "reclassified" | "expired";
export type OutcomeKind =
  | "pending"
  | "reversal_followed"
  | "continuation_followed"
  | "exhaustion_detected"
  | "absorption_detected"
  | "expectation_failed"
  | "reclaim_anomaly";
export type SweepDirection = "bullish" | "bearish";
export type CascadeDirection = "long_liq" | "short_liq";

export interface HtfLevel {
  interval: string;
  level_price: string;
  age_candles: number;
}

export interface OutcomeDetail {
  magnitude_pct: string | null;
  duration_ms: number | null;
  max_extension: string | null;
  failure_note: string | null;
}

export interface MarketEvent {
  id: number | null;
  coin: string;
  interval: string;
  event_type: EventType;
  lifecycle: EventLifecycle;
  // Sweep fields
  sweep_direction: SweepDirection | null;
  level_price: string | null;
  sweep_extreme: string | null;
  wick_pct: string | null;
  close_price: string | null;
  // Cascade fields
  cascade_direction: CascadeDirection | null;
  cascade_start_price: string | null;
  liq_count_total: number | null;
  candles_sustained: number | null;
  volume_acceleration: string | null;
  // Shared
  event_ts_ms: number;
  candle_volume: string;
  htf_confluence: HtfLevel[];
  outcome: OutcomeKind;
  outcome_detail: OutcomeDetail | null;
  outcome_resolved_ts: number | null;
  regime: string | null;
  reclassified_from: number | null;
}

interface UseMarketEventsOptions {
  coin: string;
  enabled?: boolean;
}

interface UseMarketEventsResult {
  events: MarketEvent[];
  status: WsStatus;
  clearEvents: () => void;
}

export function useMarketEvents({
  coin,
  enabled = true,
}: UseMarketEventsOptions): UseMarketEventsResult {
  const [events, setEvents] = useState<MarketEvent[]>([]);
  const [status, setStatus] = useState<WsStatus>("disconnected");

  const wsRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(INITIAL_BACKOFF);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingEventsRef = useRef<MarketEvent[]>([]);
  const enabledRef = useRef(enabled);
  const coinRef = useRef(coin);

  enabledRef.current = enabled;
  coinRef.current = coin;

  const clearEvents = useCallback(() => setEvents([]), []);

  const flushEvents = useCallback(() => {
    flushRef.current = null;
    const pending = pendingEventsRef.current;
    if (pending.length === 0) return;
    pendingEventsRef.current = [];

    startTransition(() => {
      setEvents((prev) => {
        const merged = dedupeAndSortEvents([...pending, ...prev]);
        return merged.length > MAX_EVENTS ? merged.slice(0, MAX_EVENTS) : merged;
      });
    });
  }, []);

  const scheduleEventFlush = useCallback(() => {
    if (flushRef.current) return;
    flushRef.current = setTimeout(flushEvents, FLUSH_INTERVAL_MS);
  }, [flushEvents]);

  const connect = useCallback(() => {
    if (!enabledRef.current) return;

    const url = `${WS_URL}/ws/events?coin=${encodeURIComponent(coinRef.current)}`;
    setStatus("connecting");

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      setStatus("connected");
      backoffRef.current = INITIAL_BACKOFF;
    };

    ws.onmessage = (event: MessageEvent<string>) => {
      try {
        const mEvent: MarketEvent = JSON.parse(event.data);
        pendingEventsRef.current.push(mEvent);
        scheduleEventFlush();
      } catch {
        // malformed message — ignore
      }
    };

    ws.onerror = () => setStatus("error");

    ws.onclose = () => {
      setStatus("disconnected");
      wsRef.current = null;
      if (!enabledRef.current) return;
      const delay = backoffRef.current;
      backoffRef.current = Math.min(delay * 2, MAX_BACKOFF);
      timerRef.current = setTimeout(connect, delay);
    };
  }, []);

  useEffect(() => {
    setEvents([]);
    pendingEventsRef.current = [];
    if (wsRef.current) {
      wsRef.current.onclose = null;
      wsRef.current.close();
      wsRef.current = null;
    }
    backoffRef.current = INITIAL_BACKOFF;
    if (enabled) connect();

    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      if (flushRef.current) clearTimeout(flushRef.current);
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
      }
    };
  }, [coin, enabled, connect]);

  return { events, status, clearEvents };
}

// ---------------------------------------------------------------------------
// useLiveTrades (existing)
// ---------------------------------------------------------------------------

export function useLiveTrades({
  coin,
  enabled = true,
  maxItems = MAX_TRADES,
}: UseLiveTradesOptions): UseLiveTradesResult {
  const [trades, setTrades] = useState<Trade[]>([]);
  const [status, setStatus] = useState<WsStatus>("disconnected");

  const wsRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(INITIAL_BACKOFF);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingTradesRef = useRef<Trade[]>([]);
  const enabledRef = useRef(enabled);
  const coinRef = useRef(coin);
  const maxItemsRef = useRef(maxItems);

  // Keep refs in sync so the closure always reads the latest value
  enabledRef.current = enabled;
  coinRef.current = coin;
  maxItemsRef.current = maxItems;

  const clearTrades = useCallback(() => setTrades([]), []);

  const flushTrades = useCallback(() => {
    flushRef.current = null;
    const pending = pendingTradesRef.current;
    if (pending.length === 0) return;
    pendingTradesRef.current = [];

    startTransition(() => {
      setTrades((prev) => {
        const merged = dedupeAndSortTrades([...pending, ...prev]);
        return merged.length > maxItemsRef.current
          ? merged.slice(0, maxItemsRef.current)
          : merged;
      });
    });
  }, []);

  const scheduleTradeFlush = useCallback(() => {
    if (flushRef.current) return;
    flushRef.current = setTimeout(flushTrades, FLUSH_INTERVAL_MS);
  }, [flushTrades]);

  const connect = useCallback(() => {
    if (!enabledRef.current) return;

    const url = `${WS_URL}/ws/trades?coin=${encodeURIComponent(coinRef.current)}`;
    setStatus("connecting");

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      setStatus("connected");
      backoffRef.current = INITIAL_BACKOFF;
    };

    ws.onmessage = (event: MessageEvent<string>) => {
      try {
        const trade: Trade = JSON.parse(event.data);
        pendingTradesRef.current.push(trade);
        scheduleTradeFlush();
      } catch {
        // malformed message — ignore
      }
    };

    ws.onerror = () => {
      setStatus("error");
    };

    ws.onclose = () => {
      setStatus("disconnected");
      wsRef.current = null;

      if (!enabledRef.current) return;

      const delay = backoffRef.current;
      backoffRef.current = Math.min(delay * 2, MAX_BACKOFF);
      timerRef.current = setTimeout(connect, delay);
    };
  }, []);

  useEffect(() => {
    // Reset and reconnect when the coin changes
    setTrades([]);
    pendingTradesRef.current = [];
    if (wsRef.current) {
      wsRef.current.onclose = null; // prevent auto-reconnect on intentional close
      wsRef.current.close();
      wsRef.current = null;
    }
    backoffRef.current = INITIAL_BACKOFF;

    if (enabled) {
      connect();
    }

    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      if (flushRef.current) clearTimeout(flushRef.current);
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
      }
    };
  }, [coin, enabled, connect]);

  return { trades, status, clearTrades };
}

function dedupeAndSortTrades(trades: Trade[]): Trade[] {
  const byHash = new Map<string, Trade>();
  for (const trade of trades) {
    byHash.set(trade.trade_hash, trade);
  }

  return Array.from(byHash.values()).sort((a, b) => {
    if (b.timestamp_ms !== a.timestamp_ms) return b.timestamp_ms - a.timestamp_ms;
    return b.trade_hash.localeCompare(a.trade_hash);
  });
}

function dedupeAndSortEvents(events: MarketEvent[]): MarketEvent[] {
  const byKey = new Map<string, MarketEvent>();
  for (const event of events) {
    const key = event.id !== null ? `id:${event.id}` : `${event.coin}:${event.interval}:${event.event_ts_ms}:${event.event_type}`;
    byKey.set(key, event);
  }

  return Array.from(byKey.values()).sort((a, b) => b.event_ts_ms - a.event_ts_ms);
}
