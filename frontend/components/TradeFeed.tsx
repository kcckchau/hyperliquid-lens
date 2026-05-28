"use client";

import { memo, useDeferredValue } from "react";
import type { Trade, WsStatus } from "@/lib/ws";
import { formatPrice, formatSize, formatTime } from "@/lib/api";
import { Zap } from "lucide-react";

interface TradeFeedProps {
  trades: Trade[];
  status: WsStatus;
}

const STATUS_CONFIG = {
  connected: { dot: "bg-accent animate-pulse", label: "LIVE", color: "text-accent" },
  connecting: { dot: "bg-yellow-400 animate-pulse", label: "CONNECTING", color: "text-yellow-400" },
  disconnected: { dot: "bg-muted", label: "OFFLINE", color: "text-text-secondary" },
  error: { dot: "bg-sell-red", label: "ERROR", color: "text-sell-red" },
};

export default function TradeFeed({ trades, status }: TradeFeedProps) {
  const statusCfg = STATUS_CONFIG[status];
  const deferredTrades = useDeferredValue(trades);

  return (
    <div className="flex flex-col h-full bg-surface border border-border rounded overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-2">
          <Zap size={14} className="text-accent" />
          <span className="text-text-primary text-xs font-semibold tracking-widest uppercase">
            Live Trades
          </span>
        </div>
        <div className={`flex items-center gap-1.5 ${statusCfg.color}`}>
          <span className={`w-1.5 h-1.5 rounded-full ${statusCfg.dot}`} />
          <span className="text-xs font-semibold tracking-wider">{statusCfg.label}</span>
        </div>
      </div>

      {/* Column headers */}
      <div className="grid grid-cols-[60px_1fr_1fr_80px] gap-2 px-4 py-1.5 border-b border-border">
        <span className="text-text-secondary text-[10px] uppercase tracking-widest">Side</span>
        <span className="text-text-secondary text-[10px] uppercase tracking-widest text-right">Price</span>
        <span className="text-text-secondary text-[10px] uppercase tracking-widest text-right">Size</span>
        <span className="text-text-secondary text-[10px] uppercase tracking-widest text-right">Time</span>
      </div>

      {/* Trades list */}
      <div className="flex-1 overflow-y-auto">
        {deferredTrades.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <span className="text-text-secondary text-xs">
              {status === "connecting" ? "Connecting to feed…" : "Waiting for trades…"}
            </span>
          </div>
        ) : (
          deferredTrades.map((trade) => (
            <TradeRow key={trade.trade_hash} trade={trade} />
          ))
        )}
      </div>

      {/* Footer count */}
      <div className="px-4 py-1.5 border-t border-border">
        <span className="text-text-secondary text-[10px]">
          {deferredTrades.length} recent trades
        </span>
      </div>
    </div>
  );
}

const TradeRow = memo(function TradeRow({ trade }: { trade: Trade }) {
  const isBuy = trade.side === "B";

  return (
    <div
      className={`
        grid grid-cols-[60px_1fr_1fr_80px] gap-2 px-4 py-1.5
        border-b border-border/50
        hover:bg-muted/30 transition-colors duration-75
        animate-fade-in
        ${isBuy ? "border-l-2 border-l-buy-green/30" : "border-l-2 border-l-sell-red/30"}
      `}
    >
      <span
        className={`text-xs font-semibold ${isBuy ? "text-buy-green" : "text-sell-red"}`}
      >
        {isBuy ? "BUY" : "SELL"}
        {trade.is_liquidation && (
          <span className="ml-1 text-[9px] text-yellow-400 font-bold">LIQ</span>
        )}
      </span>
      <span className={`text-xs text-right font-mono ${isBuy ? "text-buy-green" : "text-sell-red"}`}>
        {formatPrice(trade.price)}
      </span>
      <span className="text-xs text-right font-mono text-text-primary">
        {formatSize(trade.size)}
      </span>
      <span className="text-xs text-right font-mono text-text-secondary">
        {formatTime(trade.timestamp_ms)}
      </span>
    </div>
  );
});
