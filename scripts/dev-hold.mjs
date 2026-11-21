// Holds or releases the development script's rebuilds for an edit that spans
// several files (see scripts/lib/dev-hold.mjs).
//
//   node scripts/dev-hold.mjs on [--minutes N] [reason…]   hold; ends by itself after N minutes (20)
//   node scripts/dev-hold.mjs off                          release; the running script rebuilds once
//   node scripts/dev-hold.mjs                              show the hold in force
import { resolve } from "node:path";
import { DEFAULT_MINUTES, HOLD, clearHold, readHold, writeHold } from "./lib/dev-hold.mjs";
import { t } from "./lib/console-language.mjs";

const path = resolve(import.meta.dirname, "..", HOLD);
const [command, ...rest] = process.argv.slice(2);
const clock = (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

if (command === "on") {
  let minutes = DEFAULT_MINUTES;
  const at = rest.indexOf("--minutes");
  if (at >= 0) minutes = Number(rest.splice(at, 2)[1]);
  const hold = writeHold(path, minutes, rest.join(" "));
  console.log(t(`已暂停开发脚本的自动重建，至 ${clock(hold.until)} 为止。`, `Rebuilds of the development script are on hold until ${clock(hold.until)}.`));
} else if (command === "off") {
  console.log(
    clearHold(path)
      ? t("已解除暂停；期间有修改时，开发脚本随即重建一次。", "Hold released; the development script rebuilds once if anything changed.")
      : t("当前没有暂停。", "No hold was in force."),
  );
} else if (command === undefined) {
  const hold = readHold(path);
  console.log(
    hold
      ? t(`自动重建已暂停，至 ${clock(hold.until)} 为止${hold.reason ? `：${hold.reason}` : ""}。`, `Rebuilds are on hold until ${clock(hold.until)}${hold.reason ? `: ${hold.reason}` : ""}.`)
      : t("当前没有暂停。", "No hold is in force."),
  );
} else {
  console.error(t("用法：node scripts/dev-hold.mjs [on [--minutes N] [原因…] | off]", "usage: node scripts/dev-hold.mjs [on [--minutes N] [reason…] | off]"));
  process.exit(2);
}
