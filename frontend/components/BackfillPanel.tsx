"use client";

import { useState } from "react";
import { RefreshCw, DatabaseZap } from "lucide-react";
import { COINS, type Coin } from "@/components/CoinSelector";
import {
  runBackfill,
  fetchEvents,
  type BackfillSummary,
  type HistoricalEvent,
} from "@/lib/api";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function fmtPrice(val: string | null): string {
  if (!val) return "—";
  const n = parseFloat(val);
  return n >= 1000
    ? n.toLocaleString("en-US", { maximumFractionDigits: 2 })
    : n.toFixed(4);
}

function fmtPct(val: string | null): string {
  if (!val) return "—";
  return `${(parseFloat(val) * 100).toFixed(3)}%`;
}

function fmtTime(ms: number): string {
  return new Date(ms).toISOString().slice(11, 16); // HH:MM UTC
}

function yesterday(): string {
  const d = new Date();
  d.setDate(d.getDate() - 1);
  return d.toISOString().slice(0, 10);
}

function dateToMs(dateStr: string): { from: number; to: number } {
  const from = new Date(`${dateStr}T00:00:00Z`).getTime();
  return { from, to: from + 86_400_000 };
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function SummaryBar({ summary }: { summary: BackfillSummary }) {
  const max = Math.max(...summary.by_interval.map((b) => b.sweeps), 1);

  return (
    <div className="px-3 py-2.5 border-b border-border shrink-0">
      <div className="flex items-baseline justify-between mb-2">
        <span className="text-xs font-mono text-text-primary font-semibold">
          {summary.coin} · {summary.date}
        </span>
        <span className="text-[10px] text-accent font-mono">
          {summary.events_detected} events
        </span>
      </div>

      <div className="space-y-1">
        {summary.by_interval.map((b) => (
          <div key={b.interval} className="flex items-center gap-2">
            <span className="text-[10px] text-text-secondary font-mono w-6 shrink-0">
              {b.interval}
            </span>
            <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
              <div
                className="h-full bg-accent/60 rounded-full transition-all duration-500"
                style={{ width: `${(b.sweeps / max) * 100}%` }}
              />
            </div>
            <span className="text-[10px] text-text-secondary font-mono w-12 text-right shrink-0">
              {b.sweeps > 0 ? `${b.sweeps} sweep${b.sweeps !== 1 ? "s" : ""}` : "—"}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function EventRow({ event }: { event: HistoricalEvent }) {
  const isSweep = event.event_type === "liquidity_sweep";
  const isBullish =
    isSweep ? event.sweep_direction === "bullish" : event.cascade_direction === "short_liq";

  const badgeClass = isSweep
    ? isBullish
      ? "bg-emerald-500/15 text-emerald-400 border-emerald-500/30"
      : "bg-red-500/15 text-red-400 border-red-500/30"
    : isBullish
    ? "bg-purple-500/15 text-purple-300 border-purple-500/30"
    : "bg-red-700/20 text-red-300 border-red-600/30";

  const label = isSweep
    ? `${isBullish ? "Bullish" : "Bearish"} Sweep`
    : `${event.cascade_direction === "long_liq" ? "Long Liq" : "Short Liq"} Cascade`;

  return (
    <div className="px-3 py-2 border-b border-border hover:bg-surface/60 transition-colors animate-fade-in">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span
            className={`text-[10px] font-semibold px-1.5 py-0.5 rounded border leading-none ${badgeClass}`}
          >
            {label}
          </span>
          <span className="text-[10px] text-text-secondary">{event.interval}</span>
          {event.htf_confluence.length > 0 && (
            <span className="text-[9px] text-accent font-medium">
              +{event.htf_confluence.length} HTF
            </span>
          )}
        </div>
        <span className="text-[10px] text-text-secondary font-mono tabular-nums shrink-0">
          {fmtTime(event.event_ts_ms)}
        </span>
      </div>

      <div className="flex gap-3 mt-1 text-[10px]">
        {isSweep ? (
          <>
            <span className="text-text-secondary">
              Level{" "}
              <span className="text-text-primary font-mono">
                {fmtPrice(event.level_price)}
              </span>
            </span>
            <span className="text-text-secondary">
              Wick{" "}
              <span className="text-text-primary font-mono">
                {fmtPct(event.wick_pct)}
              </span>
            </span>
            <span className="text-text-secondary">
              Close{" "}
              <span className="text-text-primary font-mono">
                {fmtPrice(event.close_price)}
              </span>
            </span>
          </>
        ) : (
          <>
            <span className="text-text-secondary">
              Start{" "}
              <span className="text-text-primary font-mono">
                {fmtPrice(event.cascade_start_price)}
              </span>
            </span>
            <span className="text-text-secondary">
              Candles{" "}
              <span className="text-text-primary font-mono">
                {event.candles_sustained ?? "—"}
              </span>
            </span>
          </>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

interface BackfillPanelProps {
  defaultCoin: Coin;
  onDateChange?: (date: string | null) => void;
}

export default function BackfillPanel({ defaultCoin, onDateChange }: BackfillPanelProps) {
  const [coin, setCoin] = useState<string>(defaultCoin);
  const [date, setDate] = useState<string>(yesterday());
  const [isRunning, setIsRunning] = useState(false);
  const [summary, setSummary] = useState<BackfillSummary | null>(null);
  const [events, setEvents] = useState<HistoricalEvent[]>([]);
  const [error, setError] = useState<string | null>(null);

  async function handleRun() {
    if (!date || isRunning) return;
    setIsRunning(true);
    setError(null);
    setSummary(null);
    setEvents([]);

    try {
      const result = await runBackfill(coin, date);
      setSummary(result);

      if (result.events_detected > 0) {
        const { from, to } = dateToMs(date);
        const evts = await fetchEvents(coin, { source: "backfill", from, to, limit: 200 });
        setEvents(evts);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Unknown error");
    } finally {
      setIsRunning(false);
    }
  }

  return (
    <div className="flex flex-col h-full bg-surface border border-border rounded overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border shrink-0">
        <div className="flex items-center gap-2">
          <DatabaseZap size={12} className="text-accent" />
          <span className="text-[11px] font-semibold text-text-primary uppercase tracking-widest">
            Backfill
          </span>
        </div>
        <span className="text-[10px] text-text-secondary">Historical Detection</span>
      </div>

      {/* Controls */}
      <div className="px-3 py-3 border-b border-border shrink-0 space-y-2">
        <div className="flex gap-2">
          {/* Coin selector */}
          <select
            value={coin}
            onChange={(e) => setCoin(e.target.value)}
            disabled={isRunning}
            className="
              bg-background border border-border rounded px-2 py-1.5
              text-xs text-text-primary font-mono
              focus:border-accent focus:outline-none
              disabled:opacity-50
              [color-scheme:dark]
            "
          >
            {COINS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>

          {/* Date picker */}
          <input
            type="date"
            value={date}
            max={yesterday()}
            onChange={(e) => {
              setDate(e.target.value);
              onDateChange?.(e.target.value || null);
            }}
            disabled={isRunning}
            className="
              flex-1 bg-background border border-border rounded px-2 py-1.5
              text-xs text-text-primary font-mono
              focus:border-accent focus:outline-none
              disabled:opacity-50
              [color-scheme:dark]
            "
          />
        </div>

        <button
          onClick={handleRun}
          disabled={isRunning || !date}
          className="
            w-full flex items-center justify-center gap-2
            py-1.5 text-[10px] uppercase tracking-widest rounded border
            transition-colors
            bg-accent/10 border-accent/40 text-accent
            hover:bg-accent/20
            disabled:opacity-40 disabled:cursor-not-allowed
          "
        >
          {isRunning ? (
            <>
              <RefreshCw size={10} className="animate-spin" />
              Running…
            </>
          ) : (
            "Run Backfill"
          )}
        </button>
      </div>

      {/* Results */}
      <div className="flex-1 overflow-y-auto min-h-0">
        {error && (
          <div className="px-3 py-3 text-[11px] text-sell-red border-b border-border">
            {error}
          </div>
        )}

        {summary && <SummaryBar summary={summary} />}

        {summary && events.length > 0 && (
          <div className="px-3 py-2 border-b border-border shrink-0">
            <span className="text-[10px] text-text-secondary uppercase tracking-widest">
              Detected Events ({events.length})
            </span>
          </div>
        )}

        {events.length > 0 ? (
          events.map((event) => <EventRow key={event.id} event={event} />)
        ) : summary && !isRunning ? (
          <div className="flex items-center justify-center h-24 text-[11px] text-text-secondary">
            No sweeps detected for this day
          </div>
        ) : !summary && !error ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-center px-6">
            <DatabaseZap size={20} className="text-muted" />
            <span className="text-[11px] text-text-secondary">
              Select a coin and date, then run backfill to detect historical liquidity sweeps.
            </span>
          </div>
        ) : null}
      </div>
    </div>
  );
}
