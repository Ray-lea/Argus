// Retry transient API/transport failures without changing deployment targets.
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { repoRoot } from "./paths.mjs";

export function transientFailure(output) {
  const text = output.replace(/\u001b\[[0-9;]*m/g, "");
  if (/authentication error|unauthorized|forbidden|(?:HTTP|status(?: code)?|->)\s*[:=]?\s*(?:401|403)\b|code:\s*10000\b/i.test(text)) return false;
  return /\b(?:429 Too Many Requests|500 Internal Server Error|502 Bad Gateway|503 Service Unavailable|504 Gateway Timeout|520 Unknown Error|522 Connection Timed Out|524 A Timeout Occurred)\b|\b(?:ETIMEDOUT|ECONNRESET|EAI_AGAIN|UND_ERR_CONNECT_TIMEOUT)\b/i.test(text);
}

export async function deployWithRetry(run, { wait = delay, attempts = 3, report = console.warn } = {}) {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const result = await run();
    if (result.code === 0) return;
    if (attempt === attempts || !transientFailure(result.output))
      throw Error(`Wrangler deployment failed (exit ${result.code ?? "unknown"}, attempt ${attempt}/${attempts}).`);
    const pause = 5000 * 3 ** (attempt - 1);
    report(`[website] Temporary deployment failure; retrying in ${pause / 1000}s (${attempt + 1}/${attempts}).`);
    await wait(pause);
  }
}

async function main() {
  const [service, ...extra] = process.argv.slice(2);
  if (!["downloads", "updates", "site", "www"].includes(service) || extra.some(arg => arg !== "--dry-run"))
    throw Error("Usage: deploy.mjs downloads|updates|site|www [--dry-run]");
  const wrangler = process.env.WRANGLER;
  if (!/^wrangler@\d+\.\d+\.\d+$/.test(wrangler ?? ""))
    throw Error("WRANGLER must name the pinned version, e.g. wrangler@4.156.2.");
  await deployWithRetry(() => new Promise((resolve, reject) => {
    const child = spawn(process.platform === "win32" ? "pnpm.cmd" : "pnpm", ["dlx", wrangler, "deploy", "--config", `website/cloudflare/${service}.jsonc`, ...extra], {
      cwd: repoRoot, windowsHide: true, shell: process.platform === "win32", stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    for (const [stream, destination] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
      stream.on("data", chunk => {
        destination.write(chunk);
        output = (output + chunk.toString()).slice(-128000);
      });
    }
    child.once("error", reject);
    child.once("close", code => resolve({ code, output }));
  }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
