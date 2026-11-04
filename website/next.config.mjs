import { createMDX } from "fumadocs-mdx/next";

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
export default withMDX({
  // A static site: Cloudflare serves the exported files, there is no server.
  output: "export",
  trailingSlash: true,
  reactStrictMode: true,
  // Builds started while a development server runs keep their own cache.
  distDir: process.env.ARGUS_NEXT_DIR || ".next",
  images: { unoptimized: true },
  poweredByHeader: false,
  // The repository keeps its own agent instructions in AGENTS.md.
  agentRules: false,
  productionBrowserSourceMaps: false,
  // Chinese and English pages have separate root layouts; one 404 serves both.
  experimental: { globalNotFound: true },
});
