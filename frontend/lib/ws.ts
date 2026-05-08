"use client";

import { useEffect, useRef, useState, useCallback } from "react";

export interface Trade {
  coin: string;
  side: "B" | "S";
  price: string;
  size: string;
  timestamp_ms: number;
  trade_hash: string;
  is_liquidation: boolean;
}

type WsStatus = "connecting" | "connected" | "disconnected" | "error";

const WS_URL =
  process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:3001";

const INITIAL_BACKOFF = 1_000;
const MAX_BACKOFF = 30_000;
const MAX_TRADES = 200;

interface UseLiveTradesOptions {
  coin: string;
  enabled?: boolean;
}

interface UseLiveTradesResult {
  trades: Trade[];
  status: WsStatus;
  clearTrades: () => void;
}

export function useLiveTrades({
  coin,
  enabled = true,
}: UseLiveTradesOptions): UseLiveTradesResult {
  const [trades, setTrades] = useState<Trade[]>([]);
  const [status, setStatus] = useState<WsStatus>("disconnected");

  const wsRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(INITIAL_BACKOFF);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const enabledRef = useRef(enabled);
  const coinRef = useRef(coin);

  // Keep refs in sync so the closure always reads the latest value
  enabledRef.current = enabled;
  coinRef.current = coin;

  const clearTrades = useCallback(() => setTrades([]), []);

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
        setTrades((prev) => {
          const next = [trade, ...prev];
          return next.length > MAX_TRADES ? next.slice(0, MAX_TRADES) : next;
        });
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
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
      }
    };
  }, [coin, enabled, connect]);

  return { trades, status, clearTrades };
}
