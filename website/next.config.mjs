import { createMDX } from "fumadocs-mdx/next";
import { networkInterfaces } from "node:os";

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
export default withMDX({
  // A static site: Cloudflare serves the exported files, there is no server.
  output: "export",
  trailingSlash: true,
  reactStrictMode: true,
  // The dev client waits for HMR before hydration; allow this machine's LAN
  // and virtual-interface addresses as well as Next.js's localhost default.
  allowedDevOrigins: [...new Set(Object.values(networkInterfaces()).flat().filter(Boolean).map(address => address.address))],
  // Builds started while a development server runs keep their own cache.
  distDir: process.env.ARGUS_NEXT_DIR || ".next",
  images: { unoptimized: true },
  poweredByHeader: false,
  // The repository keeps its own agent instructions in AGENTS.md.
  agentRules: false,
  productionBrowserSourceMaps: false,
  // Language routes have separate root layouts and share the fallback page.
  experimental: { globalNotFound: true },
});
