"use client";

import { type MarketEvent, type WsStatus } from "@/lib/ws";

interface EventFeedProps {
  events: MarketEvent[];
  status: string;
}

// ── Formatting helpers ────────────────────────────────────────────────────────

function fmtPct(val: string | null): string {
  if (!val) return "—";
  return `${(parseFloat(val) * 100).toFixed(3)}%`;
}

function fmtPrice(val: string | null): string {
  if (!val) return "—";
  const n = parseFloat(val);
  return n >= 1000
    ? n.toLocaleString("en-US", { maximumFractionDigits: 2 })
    : n.toFixed(4);
}

function fmtTime(ms: number): string {
  return new Date(ms).toISOString().slice(11, 19); // HH:MM:SS UTC
}

// ── Colour coding ─────────────────────────────────────────────────────────────

function eventBadgeClass(event: MarketEvent): string {
  if (event.event_type === "liquidity_sweep") {
    return event.sweep_direction === "bullish"
      ? "bg-emerald-500/15 text-emerald-400 border-emerald-500/30"
      : "bg-red-500/15 text-red-400 border-red-500/30";
  }
  // cascade
  return event.cascade_direction === "long_liq"
    ? "bg-red-700/20 text-red-300 border-red-600/30"
    : "bg-purple-500/15 text-purple-300 border-purple-500/30";
}

function outcomeClass(outcome: string): string {
  switch (outcome) {
    case "reversal_followed":
      return "text-emerald-400";
    case "continuation_followed":
      return "text-amber-400";
    case "exhaustion_detected":
      return "text-sky-400";
    case "absorption_detected":
      return "text-indigo-400";
    case "expectation_failed":
    case "reclaim_anomaly":
      return "text-rose-400";
    default:
      return "text-text-secondary";
  }
}

function lifecycleDot(lifecycle: string): string {
  switch (lifecycle) {
    case "detected":
    case "confirming":
      return "bg-amber-400 animate-pulse";
    case "confirmed":
      return "bg-emerald-400";
    case "reclassified":
      return "bg-purple-400";
    case "expired":
      return "bg-zinc-500";
    default:
      return "bg-zinc-600";
  }
}

// ── Event label text ──────────────────────────────────────────────────────────

function eventLabel(event: MarketEvent): string {
  if (event.event_type === "liquidity_sweep") {
    const dir = event.sweep_direction === "bullish" ? "Bullish" : "Bearish";
    return `${dir} Sweep`;
  }
  const dir = event.cascade_direction === "long_liq" ? "Long Liq" : "Short Liq";
  return `${dir} Cascade`;
}

function outcomeLabel(outcome: string): string {
  return outcome.replace(/_/g, " ");
}

// ── HTF confluence indicator ──────────────────────────────────────────────────

function HtfBadges({ levels }: { levels: MarketEvent["htf_confluence"] }) {
  if (!levels.length) return null;
  return (
    <div className="flex gap-1 flex-wrap mt-0.5">
      {levels.map((l, i) => (
        <span
          key={i}
          className="text-[9px] px-1 py-0.5 rounded border border-accent/30 text-accent bg-accent/10 leading-none"
        >
          {l.interval}
        </span>
      ))}
    </div>
  );
}

// ── Single event row ──────────────────────────────────────────────────────────

function EventRow({ event }: { event: MarketEvent }) {
  const isSweep = event.event_type === "liquidity_sweep";

  return (
    <div className="px-3 py-2.5 border-b border-border hover:bg-surface/60 transition-colors">
      {/* Row 1: badge + time + interval + lifecycle */}
      <div className="flex items-center justify-between gap-2 mb-1">
        <div className="flex items-center gap-1.5">
          <span
            className={`text-[10px] font-semibold px-1.5 py-0.5 rounded border leading-none ${eventBadgeClass(event)}`}
          >
            {eventLabel(event)}
          </span>
          <span className="text-[10px] text-text-secondary">{event.interval}</span>
          {event.htf_confluence.length > 0 && (
            <span className="text-[9px] text-accent font-medium">
              +{event.htf_confluence.length} HTF
            </span>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          <span
            className={`inline-block w-1.5 h-1.5 rounded-full ${lifecycleDot(event.lifecycle)}`}
          />
          <span className="text-[10px] text-text-secondary tabular-nums">
            {fmtTime(event.event_ts_ms)}
          </span>
        </div>
      </div>

      {/* Row 2: key prices */}
      <div className="flex gap-4 text-[10px]">
        {isSweep ? (
          <>
            <span className="text-text-secondary">
              Level <span className="text-text-primary font-mono">{fmtPrice(event.level_price)}</span>
            </span>
            <span className="text-text-secondary">
              Wick <span className="text-text-primary font-mono">{fmtPct(event.wick_pct)}</span>
            </span>
            <span className="text-text-secondary">
              Close <span className="text-text-primary font-mono">{fmtPrice(event.close_price)}</span>
            </span>
          </>
        ) : (
          <>
            <span className="text-text-secondary">
              Start <span className="text-text-primary font-mono">{fmtPrice(event.cascade_start_price)}</span>
            </span>
            <span className="text-text-secondary">
              Candles <span className="text-text-primary font-mono">{event.candles_sustained ?? "—"}</span>
            </span>
            <span className="text-text-secondary">
              Liqs <span className="text-text-primary font-mono">{event.liq_count_total ?? "—"}</span>
            </span>
          </>
        )}
      </div>

      {/* Row 3: outcome */}
      <div className="flex items-center justify-between mt-1">
        <span className={`text-[10px] font-medium capitalize ${outcomeClass(event.outcome)}`}>
          {outcomeLabel(event.outcome)}
          {event.outcome_detail?.magnitude_pct && (
            <span className="ml-1 text-text-secondary font-normal">
              ({fmtPct(event.outcome_detail.magnitude_pct)})
            </span>
          )}
        </span>
        {event.outcome_detail?.failure_note && (
          <span className="text-[9px] text-text-secondary italic max-w-[160px] truncate text-right">
            {event.outcome_detail.failure_note}
          </span>
        )}
      </div>

      {/* Row 4: HTF confluence */}
      <HtfBadges levels={event.htf_confluence} />
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export default function EventFeed({ events, status }: EventFeedProps) {
  return (
    <div className="flex flex-col h-full bg-surface border border-border rounded overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border bg-surface shrink-0">
        <span className="text-[11px] font-semibold text-text-primary uppercase tracking-widest">
          Market Events
        </span>
        <div className="flex items-center gap-2">
          <span className="text-[10px] text-text-secondary">{events.length} events</span>
          <div
            className={`w-1.5 h-1.5 rounded-full ${
              status === "connected"
                ? "bg-emerald-400"
                : status === "connecting"
                ? "bg-amber-400 animate-pulse"
                : "bg-zinc-500"
            }`}
          />
        </div>
      </div>

      {/* Legend */}
      <div className="flex gap-3 px-3 py-1.5 border-b border-border text-[9px] text-text-secondary uppercase tracking-wider shrink-0">
        <span className="text-emerald-400">Bullish Sweep</span>
        <span className="text-red-400">Bearish Sweep</span>
        <span className="text-red-300">Long Liq Cascade</span>
        <span className="text-purple-300">Short Liq Cascade</span>
      </div>

      {/* Event list */}
      <div className="flex-1 overflow-y-auto min-h-0">
        {events.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[11px] text-text-secondary">
            {status === "connected" ? "Waiting for events…" : "Disconnected"}
          </div>
        ) : (
          events.map((event, i) => <EventRow key={event.id ?? i} event={event} />)
        )}
      </div>
    </div>
  );
}
