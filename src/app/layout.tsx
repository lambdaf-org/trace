import './globals.css';
import type { ReactNode } from 'react';

export const metadata = {
  title: 'Trace',
  description: 'Local-only computer activity receipts for builders.',
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
