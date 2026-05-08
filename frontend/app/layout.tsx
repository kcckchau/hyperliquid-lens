import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Hyperliquid Lens",
  description: "Real-time Hyperliquid DEX trade indexer and live dashboard",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="min-h-screen bg-background text-text-primary font-mono antialiased">
        {children}
      </body>
    </html>
  );
}
