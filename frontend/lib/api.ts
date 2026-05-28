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
