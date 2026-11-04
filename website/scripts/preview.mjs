// Serves the last build (target/official-web/dist/site) the way Cloudflare
// does: folders answer with index.html, other paths with 404.html.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { workDir } from "./paths.mjs";

const root = join(workDir, "dist", "site");
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
  ".xml": "application/xml",
};
const args = process.argv.slice(2);
const at = args.findIndex((arg) => arg === "--port" || arg === "-p");
const port = Number(at >= 0 ? args[at + 1] : 4321);

async function resolve(pathname) {
  const path = normalize(join(root, decodeURIComponent(pathname)));
  if (!path.startsWith(root)) return null;
  try {
    const info = await stat(path);
    if (info.isFile()) return path;
    if (info.isDirectory()) return (await stat(join(path, "index.html"))).isFile() ? join(path, "index.html") : null;
  } catch {
    return null;
  }
  return null;
}

createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://localhost");
  if (!url.pathname.endsWith("/") && !extname(url.pathname) && (await resolve(url.pathname + "/"))) {
    response.writeHead(308, { location: url.pathname + "/" + url.search });
    return response.end();
  }
  const file = await resolve(url.pathname);
  const target = file ?? join(root, "404.html");
  response.writeHead(file ? 200 : 404, {
    "content-type": types[extname(target)] ?? "application/octet-stream",
  });
  response.end(await readFile(target));
}).listen(port, () => console.log(`[website] Serving ${root} at http://localhost:${port}/`));
