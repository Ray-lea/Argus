import { Show } from "solid-js";
import { fmtCount, fmtGB, fmtMem, fmtUptime } from "../lib/format";
import { t } from "../lib/i18n";
import { privilege } from "../lib/privilege";
import { appInfo, intervalMs, orderHeldByPointer, paused, snapshot } from "../lib/store";
import { ShieldIcon } from "./icons";

export default function StatusBar() {
  const sys = () => snapshot()?.system;
  return (
    <footer class="statusbar">
      {/* Figures that do not fit drop out whole, from the last one. */}
      <div class="statusbar-figures">
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
      </div>
      <Show when={orderHeldByPointer()}>
        <span title={t("shell.status.orderHeldHint")}>{t("shell.status.orderHeld")}</span>
      </Show>
      <span>
        <span class="live-dot" classList={{ paused: paused() }} />
        <Show when={!paused()} fallback={t("shell.status.paused")}>
          {t("shell.status.interval", { seconds: intervalMs() / 1000 })}
        </Show>
      </span>
      <Show when={appInfo()}>
        <span class="badge" data-privilege={privilege()} title={t(`shell.elevation.${privilege()}Hint`)}>
          <ShieldIcon size={12} />
          {t(`shell.elevation.${privilege()}`)}
        </span>
      </Show>
    </footer>
  );
}
