// PVE control for the explicitly assigned lab VM range. Credentials stay local.
import assert from "node:assert/strict";
import { request } from "node:https";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const configPath = process.env.ARGUS_LAB_CONFIG || join(homedir(), ".config/argus-lab/config.json");
const config = JSON.parse(readFileSync(configPath));
const privateFolder = dirname(configPath);
const token = JSON.parse(readFileSync(join(privateFolder, "pve-token.json")));
const ca = readFileSync(join(privateFolder, "pve-root-ca.pem"));
assert(config.url.startsWith("https://") && /^[a-zA-Z0-9_-]+$/.test(config.node));
export function pveRequest(path, method = "GET", data) {
  return new Promise((done, reject) => {
    const body = data === undefined ? undefined : JSON.stringify(data);
    const req = request(new URL(`/api2/json${path}`, config.url), { method, ca, headers: {
      Authorization: `PVEAPIToken=${token["full-tokenid"]}=${token.value}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body) }),
    } }, response => {
      const chunks = []; let size = 0;
      response.on("data", chunk => { size += chunk.length; if (size > 16 * 1024 ** 2) req.destroy(new Error("Response is too large")); else chunks.push(chunk); });
      response.on("end", () => {
        try {
          const value = JSON.parse(Buffer.concat(chunks).toString("utf8"));
          if (response.statusCode >= 400) reject(new Error(`PVE HTTP ${response.statusCode}: ${value.message || JSON.stringify(value.errors || {})}`));
          else done(value.data);
        } catch (error) { reject(error); }
      });
    });
    req.setTimeout(30000, () => req.destroy(new Error("PVE request timed out")));
    req.on("error", reject);
    req.end(body);
  });
}
export async function labCommand(operation, id, args = []) {
  const vmid = Number(id);
  assert(Number.isInteger(vmid) && vmid >= 120 && vmid <= 130, "Lab VM IDs are restricted to 120-130");
  const base = `/nodes/${config.node}/qemu/${vmid}`;
  if (operation === "task") {
    const parts = String(args[0]).split(":");
    assert(parts[0] === "UPID" && parts[1] === config.node && parts[6] === String(vmid));
    return pveRequest(`/nodes/${config.node}/tasks/${encodeURIComponent(args[0])}/status`);
  }
  if (["run", "run-script"].includes(operation)) {
    const started = await labCommand(operation === "run" ? "exec" : "exec-script", id, args);
    const deadline = Date.now() + 25000;
    while (Date.now() < deadline) {
      const result = await labCommand("exec-status", id, [String(started.pid)]);
      if (result.exited) return result;
      await new Promise(done => setTimeout(done, 300));
    }
    return { pid: started.pid, exited: false };
  }
  if (operation === "status") return pveRequest(`${base}/status/current`);
  if (operation === "config") return pveRequest(`${base}/config`);
  if (operation === "set-display") {
    assert(["std", "qxl", "virtio"].includes(args[0]));
    return pveRequest(`${base}/config`, "PUT", { vga: `${args[0]},memory=32` });
  }
  if (operation === "utc-clock") {
    const script = "$ErrorActionPreference='Stop'; New-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\TimeZoneInformation' -Name RealTimeIsUniversal -Value 1 -PropertyType DWord -Force | Out-Null";
    const result = await labCommand("run", vmid, ["powershell.exe", "-NoProfile", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")]);
    assert(result.exited && result.exitcode === 0, "Guest UTC clock configuration failed");
    await pveRequest(`${base}/config`, "PUT", { localtime: 0 });
    return { powerCycleRequired: true };
  }
  if (operation === "addresses") return pveRequest(`${base}/agent/network-get-interfaces`);
  if (["start", "shutdown", "reboot", "stop"].includes(operation)) return pveRequest(`${base}/status/${operation}`, "POST");
  if (operation === "exec") { assert(args.length); return pveRequest(`${base}/agent/exec`, "POST", { command: args }); }
  if (operation === "exec-script") {
    assert(args.length === 1);
    const encoded = Buffer.from(readFileSync(resolve(args[0]), "utf8"), "utf16le").toString("base64");
    return pveRequest(`${base}/agent/exec`, "POST", { command: ["powershell.exe", "-NoProfile", "-EncodedCommand", encoded] });
  }
  if (operation === "exec-status") { assert(/^\d+$/.test(args[0])); return pveRequest(`${base}/agent/exec-status?pid=${args[0]}`); }
  throw new Error("Use status, config, set-display, addresses, start, shutdown, reboot, stop, exec, exec-script, exec-status, run or run-script");
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { console.log(JSON.stringify(await labCommand(...process.argv.slice(2, 4), process.argv.slice(4)), null, 2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
