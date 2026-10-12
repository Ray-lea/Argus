import { Show } from "solid-js";
import { fmtCount, fmtGB, fmtMem, fmtUptime } from "../lib/format";
import { intervalMs, orderHeld, paused, snapshot } from "../lib/store";

export default function StatusBar() {
  const sys = () => snapshot()?.system;
  return (
    <footer class="statusbar">
      <span>
        线程 <b>{fmtCount(sys()?.threadCount ?? 0)}</b>
      </span>
      <span>
        句柄 <b>{fmtCount(sys()?.handleCount ?? 0)}</b>
      </span>
      <span>
        缓存 <b>{fmtGB(sys()?.cached ?? 0)} GB</b>
      </span>
      <span>
        分页缓冲池 <b>{fmtMem(sys()?.kernelPaged ?? 0)}</b>
      </span>
      <span>
        已运行 <b>{fmtUptime(sys()?.uptimeMs ?? 0)}</b>
      </span>
      <span class="spacer" />
      <Show when={orderHeld()}>
        <span title="鼠标移出列表后按当前排序重新排列">指针在列表上，暂不重新排序</span>
      </Show>
      <span>
        <span class="live-dot" classList={{ paused: paused() }} />
        <Show when={!paused()} fallback="已暂停刷新">
          每 {intervalMs() / 1000} 秒刷新
        </Show>
      </span>
    </footer>
  );
}
