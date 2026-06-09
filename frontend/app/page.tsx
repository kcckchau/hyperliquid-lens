"use client";

import { useState } from "react";
import dynamic from "next/dynamic";
import CoinSelector, { type Coin } from "@/components/CoinSelector";
import TradeFeed from "@/components/TradeFeed";
import StatsBar from "@/components/StatsBar";
import EventFeed from "@/components/EventFeed";
import BackfillPanel from "@/components/BackfillPanel";
import { useLiveTrades, useMarketEvents } from "@/lib/ws";

// Dynamically import the chart to avoid SSR issues with lightweight-charts
const CandlestickChart = dynamic(() => import("@/components/CandlestickChart"), {
  ssr: false,
  loading: () => (
    <div className="flex items-center justify-center h-full bg-surface border border-border rounded">
      <span className="text-text-secondary text-xs animate-pulse">Loading chart…</span>
    </div>
  ),
});

type RightPanel = "trades" | "events" | "backfill";

export default function DashboardPage() {
  const [coin, setCoin] = useState<Coin>("BTC");
  const [rightPanel, setRightPanel] = useState<RightPanel>("events");
  const [chartDateRange, setChartDateRange] = useState<{ from: number; to: number } | null>(null);

  function handleCoinChange(next: Coin) {
    setCoin(next);
    setChartDateRange(null); // reset to live when coin changes
  }

  function handleBackfillDateChange(date: string | null) {
    if (!date) { setChartDateRange(null); return; }
    const from = new Date(`${date}T00:00:00Z`).getTime();
    setChartDateRange({ from, to: from + 86_400_000 });
  }

  const { trades, status: tradeStatus } = useLiveTrades({ coin });
  const { events, status: eventStatus } = useMarketEvents({ coin });

  return (
    <div className="flex flex-col h-screen overflow-hidden">
      {/* Top nav */}
      <header className="flex items-center justify-between px-6 py-3 border-b border-border bg-surface">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div className="w-6 h-6 rounded bg-accent/20 border border-accent/40 flex items-center justify-center">
              <span className="text-accent text-[10px] font-bold">HL</span>
            </div>
            <span className="text-text-primary text-sm font-semibold tracking-wider">
              HYPERLIQUID
            </span>
            <span className="text-text-secondary text-sm tracking-wider">LENS</span>
          </div>

          <div className="w-px h-4 bg-border" />

          <CoinSelector value={coin} onChange={handleCoinChange} />
        </div>

        <div className="flex items-center gap-4 text-[10px] text-text-secondary uppercase tracking-widest">
          <span>Hyperliquid DEX</span>
          <div className="w-px h-3 bg-border" />
          <span>v0.1.0</span>
        </div>
      </header>

      {/* Stats bar */}
      <div className="px-6 py-3 border-b border-border">
        <StatsBar coin={coin} liveTrades={trades} />
      </div>

      {/* Main content grid */}
      <main className="flex-1 grid grid-cols-[1fr_360px] gap-4 p-4 overflow-hidden min-h-0">
        {/* Chart — left, full height */}
        <div className="min-h-0">
          <CandlestickChart
            coin={coin}
            dateRange={chartDateRange}
            onBackToLive={() => setChartDateRange(null)}
          />
        </div>

        {/* Right panel — toggle between events and trades */}
        <div className="flex flex-col min-h-0 gap-2">
          {/* Panel toggle */}
          <div className="flex gap-1 shrink-0">
            {(["events", "trades", "backfill"] as const).map((panel) => (
              <button
                key={panel}
                onClick={() => setRightPanel(panel)}
                className={`flex-1 text-[10px] uppercase tracking-widest py-1 rounded border transition-colors ${
                  rightPanel === panel
                    ? "bg-accent/20 border-accent/40 text-accent"
                    : "bg-transparent border-border text-text-secondary hover:text-text-primary"
                }`}
              >
                {panel}
              </button>
            ))}
          </div>

          <div className="flex-1 min-h-0">
            {rightPanel === "events" ? (
              <EventFeed events={events} status={eventStatus} />
            ) : rightPanel === "trades" ? (
              <TradeFeed trades={trades} status={tradeStatus} />
            ) : (
              <BackfillPanel
                defaultCoin={coin}
                onDateChange={handleBackfillDateChange}
              />
            )}
          </div>
        </div>
      </main>

      {/* Footer */}
      <footer className="px-6 py-2 border-t border-border text-[10px] text-text-secondary flex items-center justify-between">
        <span>Data sourced from Hyperliquid WebSocket API</span>
        <span>Prices in USDC · All times UTC</span>
      </footer>
    </div>
  );
}
