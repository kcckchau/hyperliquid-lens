"use client";

import { useEffect, useRef, useState, useCallback } from "react";
import {
  createChart,
  type IChartApi,
  type ISeriesApi,
  type CandlestickData,
  ColorType,
  CrosshairMode,
} from "lightweight-charts";
import { fetchSummary, type OhlcvCandle } from "@/lib/api";
import { useLiveTrades } from "@/lib/ws";
import { BarChart2, RefreshCw } from "lucide-react";

interface CandlestickChartProps {
  coin: string;
}

const INTERVALS = ["1m", "5m", "15m", "1h", "4h", "1d"] as const;
type Interval = (typeof INTERVALS)[number];
const INTERVAL_MS: Record<Interval, number> = {
  "1m": 60_000,
  "5m": 300_000,
  "15m": 900_000,
  "1h": 3_600_000,
  "4h": 14_400_000,
  "1d": 86_400_000,
};
const DEFAULT_VISIBLE_BARS: Record<Interval, number> = {
  "1m": 300,
  "5m": 300,
  "15m": 260,
  "1h": 240,
  "4h": 220,
  "1d": 220,
};

function candleToChartData(c: OhlcvCandle): CandlestickData {
  return {
    time: (c.bucket_ms / 1000) as CandlestickData["time"],
    open: parseFloat(c.open),
    high: parseFloat(c.high),
    low: parseFloat(c.low),
    close: parseFloat(c.close),
  };
}

export default function CandlestickChart({ coin }: CandlestickChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const latestCandleRef = useRef<CandlestickData | null>(null);

  const [interval, setChartInterval] = useState<Interval>("1h");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { trades } = useLiveTrades({ coin, maxItems: 1 });
  const latestTrade = trades[0];

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchSummary(coin, interval, DEFAULT_VISIBLE_BARS[interval]);
      const chartData = data.candles.map(candleToChartData);
      seriesRef.current?.setData(chartData);
      latestCandleRef.current = chartData.length > 0 ? chartData[chartData.length - 1] : null;
      chartRef.current?.timeScale().fitContent();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load chart data");
    } finally {
      setLoading(false);
    }
  }, [coin, interval]);

  // Initialise chart once on mount
  useEffect(() => {
    if (!containerRef.current) return;

    const chart = createChart(containerRef.current, {
      layout: {
        background: { type: ColorType.Solid, color: "#111111" },
        textColor: "#888888",
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: 11,
      },
      grid: {
        vertLines: { color: "#1f1f1f" },
        horzLines: { color: "#1f1f1f" },
      },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: { color: "#333333", labelBackgroundColor: "#1f1f1f" },
        horzLine: { color: "#333333", labelBackgroundColor: "#1f1f1f" },
      },
      rightPriceScale: {
        borderColor: "#1f1f1f",
        textColor: "#888888",
      },
      timeScale: {
        borderColor: "#1f1f1f",
        timeVisible: true,
        secondsVisible: false,
      },
      handleScroll: true,
      handleScale: true,
    });

    const series = chart.addCandlestickSeries({
      upColor: "#00ff88",
      downColor: "#ff4466",
      borderUpColor: "#00ff88",
      borderDownColor: "#ff4466",
      wickUpColor: "#00ff88",
      wickDownColor: "#ff4466",
    });

    chartRef.current = chart;
    seriesRef.current = series;

    const observer = new ResizeObserver(() => {
      if (containerRef.current) {
        chart.applyOptions({
          width: containerRef.current.clientWidth,
          height: containerRef.current.clientHeight,
        });
      }
    });
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  // Reload when coin or interval changes
  useEffect(() => {
    loadData();
  }, [loadData]);

  // Auto-refresh every 30s
  useEffect(() => {
    const id = setInterval(loadData, 30_000);
    return () => clearInterval(id);
  }, [loadData]);

  useEffect(() => {
    if (!latestTrade || !seriesRef.current) return;

    const price = parseFloat(latestTrade.price);
    if (Number.isNaN(price)) return;

    const bucketMs =
      Math.floor(latestTrade.timestamp_ms / INTERVAL_MS[interval]) * INTERVAL_MS[interval];
    const bucketSeconds = bucketMs / 1000;
    const current = latestCandleRef.current;

    if (!current || Number(current.time) < bucketSeconds) {
      const nextCandle: CandlestickData = {
        time: bucketSeconds as CandlestickData["time"],
        open: price,
        high: price,
        low: price,
        close: price,
      };
      latestCandleRef.current = nextCandle;
      seriesRef.current.update(nextCandle);
      return;
    }

    if (Number(current.time) > bucketSeconds) {
      return;
    }

    const updatedCandle: CandlestickData = {
      ...current,
      high: Math.max(current.high, price),
      low: Math.min(current.low, price),
      close: price,
    };
    latestCandleRef.current = updatedCandle;
    seriesRef.current.update(updatedCandle);
  }, [interval, latestTrade]);

  return (
    <div className="flex flex-col h-full bg-surface border border-border rounded overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-2">
          <BarChart2 size={14} className="text-accent" />
          <span className="text-text-primary text-xs font-semibold tracking-widest uppercase">
            {coin} / USDC
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* Interval tabs */}
          <div className="flex bg-background rounded border border-border">
            {INTERVALS.map((iv) => (
              <button
                key={iv}
                onClick={() => setChartInterval(iv)}
                className={`
                  px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider
                  transition-colors duration-100
                  ${iv === interval
                    ? "text-accent bg-muted"
                    : "text-text-secondary hover:text-text-primary"
                  }
                `}
              >
                {iv}
              </button>
            ))}
          </div>

          {/* Refresh button */}
          <button
            onClick={loadData}
            disabled={loading}
            className="p-1 text-text-secondary hover:text-accent transition-colors disabled:opacity-40"
            title="Refresh chart"
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {/* Chart area */}
      <div className="relative flex-1">
        <div ref={containerRef} className="w-full h-full" />

        {error && (
          <div className="absolute inset-0 flex items-center justify-center bg-surface/80">
            <span className="text-sell-red text-xs">{error}</span>
          </div>
        )}

        {loading && !error && seriesRef.current === null && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="text-text-secondary text-xs animate-pulse">Loading chart…</span>
          </div>
        )}
      </div>
    </div>
  );
}
