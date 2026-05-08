"use client";

import { useState, useRef, useEffect } from "react";
import { ChevronDown } from "lucide-react";

const COINS = ["BTC", "ETH", "SOL"] as const;
export type Coin = (typeof COINS)[number];

interface CoinSelectorProps {
  value: Coin;
  onChange: (coin: Coin) => void;
}

export default function CoinSelector({ value, onChange }: CoinSelectorProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        className="
          flex items-center gap-2 px-4 py-2
          bg-surface border border-border rounded
          text-text-primary font-mono text-sm font-semibold
          hover:border-accent transition-colors duration-150
          focus:outline-none focus:ring-1 focus:ring-accent
        "
      >
        <span className="text-accent">{value}</span>
        <span className="text-text-secondary text-xs">/ USDC</span>
        <ChevronDown
          size={14}
          className={`text-text-secondary transition-transform duration-150 ${open ? "rotate-180" : ""}`}
        />
      </button>

      {open && (
        <div
          className="
            absolute top-full left-0 mt-1 z-50
            bg-surface border border-border rounded
            shadow-lg shadow-black/50
            animate-slide-down
          "
        >
          {COINS.map((coin) => (
            <button
              key={coin}
              onClick={() => {
                onChange(coin);
                setOpen(false);
              }}
              className={`
                w-full flex items-center gap-2 px-4 py-2 text-left
                font-mono text-sm hover:bg-muted transition-colors duration-100
                ${coin === value ? "text-accent" : "text-text-primary"}
              `}
            >
              <span className="font-semibold">{coin}</span>
              <span className="text-text-secondary text-xs">/ USDC</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
