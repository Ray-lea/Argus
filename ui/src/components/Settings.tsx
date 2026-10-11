import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api } from "../lib/ipc";
import {
  balancedFrameMs,
  type ChartMode,
  chartMode,
  refreshHz,
  setChartMode,
  setUiMotion,
  uiMotion,
} from "../lib/motion";
import { appInfo, intervalMs, setIntervalMs } from "../lib/store";
import type { SettingsView } from "../lib/types";
import { toast } from "../lib/ui";

const INTERVALS = [500, 1000, 2000, 5000];
const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta"]);

/** Builds an accelerator the native hotkey parser accepts, e.g. `Ctrl+Alt+KeyK`. */
function accelerator(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(
    Boolean,
  ) as string[];
  if (mods.length === 0) return null;
  return [...mods, e.code].join("+");
}

const KEY_LABELS: Record<string, string> = {
  Super: "Win",
  Escape: "Esc",
  Space: "空格",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
};

function keyLabel(part: string): string {
  if (KEY_LABELS[part]) return KEY_LABELS[part];
  if (/^Key[A-Z]$/.test(part)) return part.slice(3);
  if (/^Digit\d$/.test(part)) return part.slice(5);
  if (/^Numpad\d$/.test(part)) return `Num ${part.slice(6)}`;
  return part;
}

function ToggleSwitch(props: { checked: boolean; label: string; onChange: (v: boolean) => void }) {
  return (
    <div class="setting-control">
      <span class="switch-label">{props.checked ? "开" : "关"}</span>
      <button
        type="button"
        role="switch"
        class="switch"
        aria-checked={props.checked}
        aria-label={props.label}
        onClick={() => props.onChange(!props.checked)}
      />
    </div>
  );
}

export default function SettingsPage() {
  const [settings, { mutate }] = createResource(() => api.settings());
  const [hotkeyError, setHotkeyError] = createSignal<string | null>(null);
  const [recording, setRecording] = createSignal(false);
  let stopRecording: (() => void) | null = null;
  onCleanup(() => stopRecording?.());

  const apply = async (change: Promise<SettingsView>, failure: string) => {
    try {
      mutate(await change);
    } catch (e) {
      toast("error", `${failure}：${String(e)}`);
    }
  };

  const saveHotkey = async (hotkey: string) => {
    setRecording(false);
    try {
      mutate(await api.setHotkey(hotkey));
      setHotkeyError(null);
    } catch (e) {
      setHotkeyError(String(e));
    }
  };

  const startRecording = () => {
    setHotkeyError(null);
    setRecording(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        stop();
      } else if (e.key === "Backspace" && !e.ctrlKey && !e.altKey) {
        stop();
        void saveHotkey("");
      } else {
        const acc = accelerator(e);
        if (acc) {
          stop();
          void saveHotkey(acc);
        }
      }
    };
    const stop = () => {
      setRecording(false);
      window.removeEventListener("keydown", onKey, true);
      stopRecording = null;
    };
    stopRecording = stop;
    window.addEventListener("keydown", onKey, true);
  };

  return (
    <div class="settings page">
      <div class="settings-inner">
        <Show when={settings()}>
          {(s) => (
            <>
              <h2>启动与后台</h2>
              <div class="setting">
                <div class="setting-text">
                  <div>登录 Windows 时启动</div>
                  <div class="setting-desc">在托盘中静默启动，需要时用快捷键或单击托盘图标打开。</div>
                </div>
                <ToggleSwitch
                  label="登录 Windows 时启动"
                  checked={s().autostart}
                  onChange={(v) => apply(api.setAutostart(v), "无法更改开机启动")}
                />
              </div>
              <div class="setting">
                <div class="setting-text">
                  <div>关闭窗口时保留在托盘</div>
                  <div class="setting-desc">
                    保持常驻，再次打开几乎瞬间完成。窗口隐藏时每 5 秒采样一次，历史曲线不会中断。
                  </div>
                </div>
                <ToggleSwitch
                  label="关闭窗口时保留在托盘"
                  checked={s().closeToTray}
                  onChange={(v) => apply(api.setCloseToTray(v), "无法保存设置")}
                />
              </div>

              <h2>快捷键</h2>
              <div class="setting">
                <div class="setting-text">
                  <div>显示或隐藏 Argus</div>
                  <div class="setting-desc">
                    在任何程序中都能使用。录制时按 Esc 取消，按 Backspace 关闭快捷键。
                  </div>
                  <Show when={hotkeyError() ?? s().hotkeyError}>
                    {(err) => <div class="setting-error">{err()}</div>}
                  </Show>
                </div>
                <div class="setting-control">
                  <span class="keycaps" aria-live="polite">
                    <Show when={!recording()} fallback={<span class="pending">按下新的组合键…</span>}>
                      <Show when={s().hotkey} fallback={<span class="pending">未设置</span>}>
                        <For each={s().hotkey.split("+")}>{(part) => <kbd>{keyLabel(part)}</kbd>}</For>
                      </Show>
                    </Show>
                  </span>
                  <button type="button" class="btn" disabled={recording()} onClick={startRecording}>
                    更改
                  </button>
                </div>
              </div>

              <h2>动画</h2>
              <div class="setting">
                <div class="setting-text">
                  <div>界面动画</div>
                  <div class="setting-desc">
                    切换页面、打开菜单和对话框时的过渡，以及滑动的选中标记。默认跟随 Windows
                    的“动画效果”设置。
                  </div>
                </div>
                <ToggleSwitch label="界面动画" checked={uiMotion()} onChange={setUiMotion} />
              </div>
              <div class="setting">
                <div class="setting-text">
                  <div>曲线动画</div>
                  <div class="setting-desc">
                    曲线沿时间轴匀速向左流动。帧率越高越顺滑，占用的 CPU 和 GPU 也越多。
                    <br />
                    流畅约占单核 10%–15%，均衡约一半；省电每次移动一个整像素，小图会一顿一顿。
                  </div>
                </div>
                <select
                  class="select"
                  aria-label="曲线动画"
                  value={chartMode()}
                  onChange={(e) => setChartMode(e.currentTarget.value as ChartMode)}
                >
                  <option value="fluid">流畅（{refreshHz()} 帧/秒）</option>
                  <option value="balanced">均衡（{Math.round(1000 / balancedFrameMs())} 帧/秒）</option>
                  <option value="pixel">省电（逐像素移动）</option>
                  <option value="off">关闭（每次采样跳一格）</option>
                </select>
              </div>

              <h2>数据</h2>
              <div class="setting">
                <div class="setting-text">
                  <div>刷新频率</div>
                  <div class="setting-desc">间隔越短曲线越细腻，Argus 自身的开销也越高。</div>
                </div>
                <select
                  class="select"
                  aria-label="刷新频率"
                  value={intervalMs()}
                  onChange={(e) => {
                    const ms = Number(e.currentTarget.value);
                    setIntervalMs(ms);
                    void api.setInterval(ms);
                  }}
                >
                  <For each={INTERVALS}>{(ms) => <option value={ms}>每 {ms / 1000} 秒</option>}</For>
                </select>
              </div>

              <h2>关于</h2>
              <div class="setting">
                <div class="setting-text">
                  <div>Argus {appInfo()?.version}</div>
                  <div class="setting-desc">
                    {appInfo()?.elevated
                      ? "正在以管理员身份运行。"
                      : "正在以标准权限运行，部分系统进程的信息与操作受限。"}
                  </div>
                </div>
                <button type="button" class="btn" onClick={() => api.quit()}>
                  退出 Argus
                </button>
              </div>
            </>
          )}
        </Show>
      </div>
    </div>
  );
}
