/** @type {import('next').NextConfig} */
// Tauri serves a STATIC frontend, so Next must export, not run a server.
module.exports = {
  output: 'export',
  images: { unoptimized: true },
  // Tauri dev server runs on a fixed port; keep assets relative for file:// loads.
  assetPrefix: process.env.NODE_ENV === 'production' ? '.' : undefined,
};
