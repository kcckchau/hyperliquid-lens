"use client";

import { useEffect, useState } from "react";
import { fetchTrades, formatPrice } from "@/lib/api";
import type { Trade } from "@/lib/ws";
import { TrendingUp, TrendingDown, Activity, DollarSign } from "lucide-react";

interface StatsBarProps {
  coin: string;
  liveTrades: Trade[];
}

interface Stats {
  lastPrice: number | null;
  priceChange: number | null;
  volume24h: number;
  tradeCount: number;
}

export default function StatsBar({ coin, liveTrades }: StatsBarProps) {
  const [stats, setStats] = useState<Stats>({
    lastPrice: null,
    priceChange: null,
    volume24h: 0,
    tradeCount: 0,
  });

  // Seed stats from REST API on mount / coin change
  useEffect(() => {
    const from = Date.now() - 24 * 60 * 60 * 1000;
    fetchTrades(coin, 1000, from)
      .then((rows) => {
        if (rows.length === 0) return;
        const volume = rows.reduce((sum, r) => sum + parseFloat(r.size) * parseFloat(r.price), 0);
        const first = rows[rows.length - 1];
        const last = rows[0];
        const lastPrice = parseFloat(last.price);
        const firstPrice = parseFloat(first.price);
        const priceChange = ((lastPrice - firstPrice) / firstPrice) * 100;
        setStats({ lastPrice, priceChange, volume24h: volume, tradeCount: rows.length });
      })
      .catch(() => {});
  }, [coin]);

  // Update last price from live feed
  useEffect(() => {
    if (liveTrades.length === 0) return;
    const latest = liveTrades[0];
    const latestPrice = parseFloat(latest.price);
    setStats((prev) => ({
      ...prev,
      lastPrice: latestPrice,
      tradeCount: prev.tradeCount + 1,
      volume24h: prev.volume24h + parseFloat(latest.price) * parseFloat(latest.size),
    }));
  }, [liveTrades]);

  const isUp = (stats.priceChange ?? 0) >= 0;

  return (
    <div className="flex items-center gap-6 px-4 py-3 bg-surface border border-border rounded">
      {/* Last price */}
      <StatItem
        icon={<DollarSign size={12} />}
        label="Last Price"
        value={stats.lastPrice ? `$${formatPrice(stats.lastPrice)}` : "—"}
        valueClass={isUp ? "text-buy-green" : "text-sell-red"}
      />

      {/* 24h change */}
      <StatItem
        icon={isUp ? <TrendingUp size={12} /> : <TrendingDown size={12} />}
        label="24h Change"
        value={
          stats.priceChange !== null
            ? `${stats.priceChange >= 0 ? "+" : ""}${stats.priceChange.toFixed(2)}%`
            : "—"
        }
        valueClass={isUp ? "text-buy-green" : "text-sell-red"}
      />

      {/* 24h volume */}
      <StatItem
        icon={<Activity size={12} />}
        label="24h Volume"
        value={
          stats.volume24h > 0
            ? `$${(stats.volume24h / 1_000_000).toFixed(2)}M`
            : "—"
        }
        valueClass="text-text-primary"
      />

      {/* Trade count */}
      <StatItem
        icon={<Activity size={12} />}
        label="Trades"
        value={stats.tradeCount > 0 ? stats.tradeCount.toLocaleString() : "—"}
        valueClass="text-text-primary"
      />
    </div>
  );
}

function StatItem({
  icon,
  label,
  value,
  valueClass,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueClass: string;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <div className="flex items-center gap-1 text-text-secondary">
        {icon}
        <span className="text-[10px] uppercase tracking-widest">{label}</span>
      </div>
      <span className={`text-sm font-semibold font-mono ${valueClass}`}>{value}</span>
    </div>
  );
}
