const API_URL = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3001";

export interface TradeRow {
  id: number;
  coin: string;
  side: "B" | "S";
  price: string;
  size: string;
  timestamp_ms: number;
  trade_hash: string;
  is_liquidation: boolean;
}

export interface OhlcvCandle {
  bucket_ms: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
}

export interface SummaryResponse {
  coin: string;
  interval: string;
  candles: OhlcvCandle[];
}

export async function fetchTrades(
  coin: string,
  limit = 100,
  from?: number,
  to?: number
): Promise<TradeRow[]> {
  const params = new URLSearchParams({ coin, limit: String(limit) });
  if (from) params.set("from", String(from));
  if (to) params.set("to", String(to));

  const res = await fetch(`${API_URL}/trades?${params}`);
  if (!res.ok) throw new Error(`Failed to fetch trades: ${res.statusText}`);
  return res.json();
}

export async function fetchSummary(
  coin: string,
  interval: string = "1h",
  visibleBars?: number,
  from?: number,
  to?: number
): Promise<SummaryResponse> {
  const params = new URLSearchParams({ coin, interval });
  if (visibleBars) params.set("visible_bars", String(visibleBars));
  if (from) params.set("from", String(from));
  if (to) params.set("to", String(to));

  const res = await fetch(`${API_URL}/trades/summary?${params}`);
  if (!res.ok) throw new Error(`Failed to fetch summary: ${res.statusText}`);
  return res.json();
}

export function formatPrice(price: string | number, decimals = 2): string {
  const n = typeof price === "string" ? parseFloat(price) : price;
  if (isNaN(n)) return "—";
  return n.toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

export function formatSize(size: string | number, decimals = 4): string {
  const n = typeof size === "string" ? parseFloat(size) : size;
  if (isNaN(n)) return "—";
  return n.toFixed(decimals);
}

export function formatTime(timestamp_ms: number): string {
  return new Date(timestamp_ms).toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

// ---------------------------------------------------------------------------
// Backfill
// ---------------------------------------------------------------------------

export interface BackfillIntervalSummary {
  interval: string;
  sweeps: number;
  cascades: number;
}

export interface BackfillSummary {
  coin: string;
  date: string;
  events_detected: number;
  by_interval: BackfillIntervalSummary[];
  notes: string[];
}

export async function runBackfill(
  coin: string,
  date: string
): Promise<BackfillSummary> {
  const res = await fetch(`${API_URL}/backfill`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ coin, date }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error((err as { error?: string }).error ?? `Backfill failed: ${res.statusText}`);
  }
  return res.json();
}

export interface HistoricalEvent {
  id: number;
  coin: string;
  interval: string;
  event_type: "liquidity_sweep" | "liquidation_cascade";
  sweep_direction: "bullish" | "bearish" | null;
  level_price: string | null;
  sweep_extreme: string | null;
  wick_pct: string | null;
  close_price: string | null;
  cascade_direction: "long_liq" | "short_liq" | null;
  cascade_start_price: string | null;
  candles_sustained: number | null;
  event_ts_ms: number;
  htf_confluence: Array<{ interval: string; level_price: string; age_candles: number }>;
  outcome: string;
  source: string;
}

export async function fetchEvents(
  coin: string,
  opts: { source?: string; from?: number; to?: number; limit?: number } = {}
): Promise<HistoricalEvent[]> {
  const params = new URLSearchParams({ coin });
  if (opts.source) params.set("source", opts.source);
  if (opts.from != null) params.set("from", String(opts.from));
  if (opts.to != null) params.set("to", String(opts.to));
  params.set("limit", String(opts.limit ?? 200));

  const res = await fetch(`${API_URL}/events?${params}`);
  if (!res.ok) throw new Error(`Failed to fetch events: ${res.statusText}`);
  return res.json();
}
