import { Show } from "solid-js";
import { fmtCount, fmtGB, fmtMem, fmtUptime } from "../lib/format";
import { t } from "../lib/i18n";
import { intervalMs, orderHeld, paused, snapshot } from "../lib/store";

export default function StatusBar() {
  const sys = () => snapshot()?.system;
  return (
    <footer class="statusbar">
      <span>
        {t("shell.status.threads")} <b>{fmtCount(sys()?.threadCount ?? 0)}</b>
      </span>
      <span>
        {t("shell.status.handles")} <b>{fmtCount(sys()?.handleCount ?? 0)}</b>
      </span>
      <span>
        {t("shell.status.cached")} <b>{fmtGB(sys()?.cached ?? 0)} GB</b>
      </span>
      <span>
        {t("shell.status.pagedPool")} <b>{fmtMem(sys()?.kernelPaged ?? 0)}</b>
      </span>
      <span>
        {t("shell.status.uptime")} <b>{fmtUptime(sys()?.uptimeMs ?? 0)}</b>
      </span>
      <span class="spacer" />
      <Show when={orderHeld()}>
        <span title={t("shell.status.orderHeldHint")}>{t("shell.status.orderHeld")}</span>
      </Show>
      <span>
        <span class="live-dot" classList={{ paused: paused() }} />
        <Show when={!paused()} fallback={t("shell.status.paused")}>
          {t("shell.status.interval", { seconds: intervalMs() / 1000 })}
        </Show>
      </span>
    </footer>
  );
}
