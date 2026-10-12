// Settings: cards grouped into sections, with a list of the sections on the
// left that follows the scroll.

import {
  children,
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  type JSX,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { PRIORITY_LABEL } from "../lib/actions";
import { fmtCache, netUnit, setNetUnit } from "../lib/format";
import { api } from "../lib/ipc";
import {
  balancedFrameMs,
  type ChartMode,
  chartMode,
  presence,
  refreshHz,
  setChartMode,
  setUiMotion,
  uiMotion,
} from "../lib/motion";
import { appInfo, focusRule, intervalMs, setFocusRule, setIntervalMs, setPage, snapshot } from "../lib/store";
import type { PriorityClass, ProcessRow, ProcessRule, RuleStats, SettingsView } from "../lib/types";
import { confirm, toast } from "../lib/ui";
import Indicator from "./Indicator";
import {
  BoltIcon,
  DismissIcon,
  DocumentIcon,
  HistoryIcon,
  InfoIcon,
  KeyboardIcon,
  PowerIcon,
  PulseIcon,
  SparkleIcon,
  StorageIcon,
  TimerIcon,
  TransferIcon,
  TrashIcon,
  TrayIcon,
} from "./icons";
import ProcIcon from "./ProcIcon";

const SECTIONS = [
  { id: "startup", label: "启动与后台", icon: PowerIcon },
  { id: "hotkey", label: "快捷键", icon: KeyboardIcon },
  { id: "motion", label: "动画", icon: SparkleIcon },
  { id: "data", label: "数据与历史", icon: HistoryIcon },
  { id: "rules", label: "规则", icon: BoltIcon },
  { id: "about", label: "关于", icon: InfoIcon },
] as const;
type SectionId = (typeof SECTIONS)[number]["id"];

/** A section to bring into view when the page opens. */
const [pendingSection, setPendingSection] = createSignal<SectionId | null>(null);

/** Opens the settings page at a section (from another page). */
export function openSettings(section: SectionId) {
  setPendingSection(section);
  setPage("settings");
}

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

const dateFmt = new Intl.DateTimeFormat("zh-CN", {
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});

/** One setting: icon, what it is, and its control on the right. */
function Setting(props: {
  icon: JSX.Element;
  title: JSX.Element;
  desc?: JSX.Element;
  error?: string | null;
  id?: string;
  class?: string;
  children?: JSX.Element;
}) {
  const desc = children(() => props.desc);
  const control = children(() => props.children);
  return (
    <div id={props.id} class={`setting ${props.class ?? ""}`}>
      <span class="setting-icon">{props.icon}</span>
      <div class="setting-text">
        <div class="setting-title">{props.title}</div>
        <Show when={desc()}>
          <div class="setting-desc">{desc()}</div>
        </Show>
        <Show when={props.error}>{(err) => <div class="setting-error">{err()}</div>}</Show>
      </div>
      <Show when={control()}>
        <div class="setting-control">{control()}</div>
      </Show>
    </div>
  );
}

function ToggleSwitch(props: { checked: boolean; label: string; onChange: (v: boolean) => void }) {
  return (
    <>
      <span class="switch-label">{props.checked ? "开" : "关"}</span>
      <button
        type="button"
        role="switch"
        class="switch"
        aria-checked={props.checked}
        aria-label={props.label}
        onClick={() => props.onChange(!props.checked)}
      />
    </>
  );
}

const PRIORITY_CHOICES: PriorityClass[] = [
  "realtime",
  "high",
  "aboveNormal",
  "normal",
  "belowNormal",
  "idle",
];

function ruleSummary(rule: ProcessRule, stats: RuleStats | undefined): string {
  if (rule.priority === null && rule.efficiency === null) return "选择要应用的优先级或效率模式。";
  if (!rule.enabled) return "已停用。";
  if (!stats || (stats.applied === 0 && stats.denied === 0)) return "还没有遇到这个程序。";
  const parts = [];
  if (stats.applied > 0) parts.push(`已应用于 ${stats.applied} 个进程`);
  if (stats.denied > 0) parts.push(`${stats.denied} 个需要以管理员身份运行 Argus 才能更改`);
  return `${parts.join("，")}。`;
}

/** Process rules: priority and efficiency mode applied whenever a program starts. */
function RulesSection(props: { rules: ProcessRule[]; onChange: (rules: ProcessRule[]) => void }) {
  const [stats, { refetch }] = createResource(() => api.ruleStats());
  const timer = setInterval(() => void refetch(), 2000);
  onCleanup(() => clearInterval(timer));
  const [draft, setDraft] = createSignal("");
  const [draftError, setDraftError] = createSignal<string | null>(null);
  // Programs that are running, for their icons.
  const running = createMemo(() => {
    const byName = new Map<string, ProcessRow>();
    for (const p of snapshot()?.processes ?? []) {
      const name = p.name.toLowerCase();
      if (!byName.has(name)) byName.set(name, p);
    }
    return byName;
  });

  const update = (id: string, patch: Partial<ProcessRule>) =>
    props.onChange(props.rules.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  const add = (e: SubmitEvent) => {
    e.preventDefault();
    const name = draft().trim();
    if (!name) return;
    if (props.rules.some((r) => r.name.toLowerCase() === name.toLowerCase())) {
      setDraftError(`已经有 ${name} 的规则。`);
      return;
    }
    const rule: ProcessRule = {
      id: crypto.randomUUID(),
      name,
      enabled: true,
      priority: null,
      efficiency: true,
    };
    props.onChange([...props.rules, rule]);
    setDraft("");
    setDraftError(null);
    queueMicrotask(() => setFocusRule(rule.id));
  };

  // Opened from the process menu: bring that rule into view once.
  createEffect(() => {
    const id = focusRule();
    if (!id || !props.rules.some((r) => r.id === id)) return;
    queueMicrotask(() => {
      const el = document.getElementById(`rule-${id}`);
      el?.scrollIntoView({ block: "center", behavior: uiMotion() ? "smooth" : "auto" });
      el?.querySelector<HTMLElement>("select")?.focus();
      setTimeout(() => setFocusRule(null), 1600);
    });
  });

  return (
    <>
      <Setting
        icon={<BoltIcon size={20} />}
        title="程序启动时自动设置优先级或效率模式"
        desc="保存后已在运行的进程也立即生效。也可以在进程列表中右键一个进程，选择“添加规则”。"
        error={draftError()}
      >
        <form class="rule-add" onSubmit={add}>
          <input
            class="input"
            placeholder="程序名，例如 chrome.exe"
            aria-label="新规则的程序名"
            value={draft()}
            onInput={(e) => {
              setDraft(e.currentTarget.value);
              setDraftError(null);
            }}
          />
          <button type="submit" class="btn" disabled={!draft().trim()}>
            添加
          </button>
        </form>
      </Setting>
      <For each={props.rules}>
        {(rule) => {
          const proc = () => running().get(rule.name.toLowerCase());
          return (
            <Setting
              id={`rule-${rule.id}`}
              class={focusRule() === rule.id ? "rule focused" : "rule"}
              icon={<ProcIcon proc={{ icon: proc()?.icon ?? null, path: proc()?.path ?? "" }} size={20} />}
              title={<span class="rule-name">{rule.name}</span>}
              desc={ruleSummary(rule, stats()?.[rule.id])}
            >
              <select
                class="select"
                aria-label={`${rule.name} 的优先级`}
                value={rule.priority ?? ""}
                onChange={(e) =>
                  update(rule.id, { priority: (e.currentTarget.value || null) as PriorityClass | null })
                }
              >
                <option value="">不改变优先级</option>
                <For each={PRIORITY_CHOICES}>
                  {(p) => <option value={p}>优先级：{PRIORITY_LABEL[p]}</option>}
                </For>
              </select>
              <select
                class="select"
                aria-label={`${rule.name} 的效率模式`}
                value={rule.efficiency === null ? "" : rule.efficiency ? "on" : "off"}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  update(rule.id, { efficiency: v === "" ? null : v === "on" });
                }}
              >
                <option value="">不改变效率模式</option>
                <option value="on">开启效率模式</option>
                <option value="off">关闭效率模式</option>
              </select>
              <button
                type="button"
                role="switch"
                class="switch"
                aria-checked={rule.enabled}
                aria-label={`启用 ${rule.name} 的规则`}
                title={rule.enabled ? "停用规则" : "启用规则"}
                onClick={() => update(rule.id, { enabled: !rule.enabled })}
              />
              <button
                type="button"
                class="btn ghost small icon"
                title="删除规则"
                aria-label={`删除 ${rule.name} 的规则`}
                onClick={() => props.onChange(props.rules.filter((r) => r.id !== rule.id))}
              >
                <DismissIcon size={14} />
              </button>
            </Setting>
          );
        }}
      </For>
    </>
  );
}

const [licenseOpen, setLicenseOpen] = createSignal(false);

/** The license text, loaded when first opened. */
function LicenseDialog() {
  const shown = presence(licenseOpen, 170);
  return (
    <Show when={shown.value()}>
      <LicenseBody leaving={shown.leaving()} />
    </Show>
  );
}

function LicenseBody(props: { leaving: boolean }) {
  const [text] = createResource(() => import("../../../LICENSE?raw").then((m) => m.default));
  const close = () => setLicenseOpen(false);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));
  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class="dialog license-dialog" role="dialog" aria-modal="true" aria-labelledby="license-title">
        <div class="dialog-body">
          <h2 id="license-title" class="dialog-title">
            GNU Affero General Public License
          </h2>
          <pre class="license-text">{text() ?? ""}</pre>
        </div>
        <div class="dialog-footer">
          <button
            type="button"
            class="btn accent"
            ref={(b) => queueMicrotask(() => b.focus())}
            onClick={close}
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}

/** Recording history, what it takes on disk, and clearing it. */
function HistorySettings(props: { enabled: boolean; onToggle: (on: boolean) => void }) {
  const [usage, { refetch }] = createResource(() => api.historyUsage());
  const [clearing, setClearing] = createSignal(false);

  const clear = async () => {
    const size = usage()?.bytes ?? 0;
    const ok = await confirm({
      title: "清除历史记录？",
      body: `将删除全部历史记录（${fmtCache(size)}），无法恢复。`,
      confirmLabel: "清除",
      danger: true,
    });
    if (!ok) return;
    setClearing(true);
    try {
      await api.clearHistory();
      toast("success", "已清除历史记录。", 2500);
    } catch (e) {
      toast("error", `无法清除历史记录：${String(e)}`);
    } finally {
      setClearing(false);
      void refetch();
    }
  };

  return (
    <>
      <Setting
        icon={<HistoryIcon size={20} />}
        title="记录历史"
        desc={
          props.enabled
            ? "每 5 秒记录一次资源占用和当时最忙的进程，只保存在这台电脑上，保留 7 天。在“历史”页查看。"
            : "已关闭。已有的记录会在 7 天后过期，也可以现在清除。"
        }
      >
        <ToggleSwitch label="记录历史" checked={props.enabled} onChange={props.onToggle} />
      </Setting>
      <Setting
        icon={<StorageIcon size={20} />}
        title="已保存的历史"
        desc={
          <Show when={usage()} fallback="正在统计…">
            {(u) =>
              u().bytes > 0 ? (
                <>
                  占用 {fmtCache(u().bytes)}
                  <Show when={u().first}>{(first) => <>，最早的记录在 {dateFmt.format(first())}</>}</Show>。
                </>
              ) : (
                "还没有记录。"
              )
            }
          </Show>
        }
      >
        <button type="button" class="btn" disabled={clearing() || !usage()?.bytes} onClick={clear}>
          <TrashIcon />
          清除
        </button>
      </Setting>
    </>
  );
}

export default function SettingsPage() {
  const [settings, { mutate }] = createResource(() => api.settings());
  const [hotkeyError, setHotkeyError] = createSignal<string | null>(null);
  const [recording, setRecording] = createSignal(false);
  let stopRecording: (() => void) | null = null;
  onCleanup(() => stopRecording?.());

  // ---- Section list: follows the scroll; a picked section stays marked until
  // the user scrolls on their own (the last ones cannot reach the top).
  let scroller!: HTMLDivElement;
  const [active, setActive] = createSignal<SectionId>(SECTIONS[0].id);
  let picked = false;
  let frame = 0;
  const sectionEl = (id: SectionId) => document.getElementById(`settings-${id}`);
  const follow = () => {
    frame = 0;
    if (picked) return;
    // A section counts once its heading passes a line about a third of the way
    // down. The line starts near the top (the first section keeps its mark
    // until the page actually moves) and over the last half screen of
    // scrolling moves to the bottom, so the last sections, which cannot reach
    // the top, still take their turn.
    const h = scroller.clientHeight;
    const y = scroller.scrollTop;
    const start = Math.min(1, y / (h / 2));
    const end = Math.max(0, 1 - (scroller.scrollHeight - h - y) / (h / 2));
    const line = scroller.getBoundingClientRect().top + h * (0.1 + 0.25 * start + 0.6 * end);
    let current: SectionId = SECTIONS[0].id;
    for (const s of SECTIONS) {
      const top = sectionEl(s.id)?.getBoundingClientRect().top;
      if (top !== undefined && top <= line) current = s.id;
    }
    if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4) {
      current = SECTIONS[SECTIONS.length - 1].id;
    }
    setActive(current);
  };
  const jump = (id: SectionId, smooth = uiMotion()) => {
    picked = true;
    setActive(id);
    sectionEl(id)?.scrollIntoView({ block: "start", behavior: smooth ? "smooth" : "auto" });
  };
  onMount(() => {
    const release = () => {
      picked = false;
    };
    for (const type of ["wheel", "pointerdown", "keydown", "touchstart"]) {
      scroller.addEventListener(type, release, { passive: true });
    }
    onCleanup(() => cancelAnimationFrame(frame));
  });
  // Arriving from another page at a given section.
  createEffect(() => {
    const id = pendingSection();
    if (!id || !settings()) return;
    setPendingSection(null);
    queueMicrotask(() => jump(id, false));
  });

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
    <div
      ref={scroller}
      class="settings page"
      onScroll={() => {
        if (!frame) frame = requestAnimationFrame(follow);
      }}
    >
      <div class="settings-layout">
        <nav class="settings-nav" aria-label="设置分组">
          <Indicator selector="[aria-current='true']" track={active} class="settings-nav-mark" />
          <For each={SECTIONS}>
            {(s) => (
              <button
                type="button"
                class="settings-nav-item"
                aria-current={active() === s.id}
                onClick={() => jump(s.id)}
              >
                <s.icon />
                <span>{s.label}</span>
              </button>
            )}
          </For>
        </nav>

        <div class="settings-content">
          <Show when={settings()}>
            {(s) => (
              <>
                <section id="settings-startup" class="settings-section">
                  <h2>启动与后台</h2>
                  <Setting
                    icon={<PowerIcon size={20} />}
                    title="登录 Windows 时启动"
                    desc="在托盘中静默启动，需要时用快捷键或单击托盘图标打开。"
                  >
                    <ToggleSwitch
                      label="登录 Windows 时启动"
                      checked={s().autostart}
                      onChange={(v) => apply(api.setAutostart(v), "无法更改开机启动")}
                    />
                  </Setting>
                  <Setting
                    icon={<TrayIcon size={20} />}
                    title="关闭窗口时保留在托盘"
                    desc="保持常驻，再次打开几乎瞬间完成。隐藏时每 5 秒采样一次，历史记录和规则照常进行。"
                  >
                    <ToggleSwitch
                      label="关闭窗口时保留在托盘"
                      checked={s().closeToTray}
                      onChange={(v) => apply(api.setCloseToTray(v), "无法保存设置")}
                    />
                  </Setting>
                </section>

                <section id="settings-hotkey" class="settings-section">
                  <h2>快捷键</h2>
                  <Setting
                    icon={<KeyboardIcon size={20} />}
                    title="显示或隐藏 Argus"
                    desc="在任何程序中都能使用。录制时按 Esc 取消，按 Backspace 关闭快捷键。"
                    error={hotkeyError() ?? s().hotkeyError}
                  >
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
                  </Setting>
                </section>

                <section id="settings-motion" class="settings-section">
                  <h2>动画</h2>
                  <Setting
                    icon={<SparkleIcon size={20} />}
                    title="界面动画"
                    desc="切换页面、打开菜单和对话框时的过渡，以及滑动的选中标记。默认跟随 Windows 的“动画效果”设置。"
                  >
                    <ToggleSwitch label="界面动画" checked={uiMotion()} onChange={setUiMotion} />
                  </Setting>
                  <Setting
                    icon={<PulseIcon size={20} />}
                    title="曲线动画"
                    desc="帧率越高越顺滑，占用也越多：流畅约占单核 10%–15%，均衡约一半；省电每次移动一个整像素，小图会一顿一顿。"
                  >
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
                  </Setting>
                </section>

                <section id="settings-data" class="settings-section">
                  <h2>数据与历史</h2>
                  <Setting
                    icon={<TimerIcon size={20} />}
                    title="刷新频率"
                    desc="间隔越短曲线越细腻，Argus 自身的开销也越高。"
                  >
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
                  </Setting>
                  <Setting
                    icon={<TransferIcon size={20} />}
                    title="网络速率单位"
                    desc="字节与磁盘速度一致，比特与网卡的链路速度、宽带套餐一致。性能页的网络视图同时显示两种。"
                  >
                    <div class="segmented" role="group" aria-label="网络速率单位">
                      <Indicator selector="[aria-pressed='true']" track={netUnit} class="seg-thumb" />
                      <button
                        type="button"
                        title="字节每秒"
                        aria-pressed={netUnit() === "bytes"}
                        onClick={() => setNetUnit("bytes")}
                      >
                        MB/s
                      </button>
                      <button
                        type="button"
                        title="比特每秒"
                        aria-pressed={netUnit() === "bits"}
                        onClick={() => setNetUnit("bits")}
                      >
                        Mbps
                      </button>
                    </div>
                  </Setting>
                  <HistorySettings
                    enabled={s().history}
                    onToggle={(on) => apply(api.setHistoryEnabled(on), "无法更改历史记录设置")}
                  />
                </section>

                <section id="settings-rules" class="settings-section">
                  <h2>规则</h2>
                  <RulesSection
                    rules={s().rules}
                    onChange={(rules) => apply(api.setRules(rules), "无法保存规则")}
                  />
                </section>

                <section id="settings-about" class="settings-section">
                  <h2>关于</h2>
                  <div class="setting about">
                    <img class="about-logo" src="/logo.svg" alt="" width="48" height="48" />
                    <div class="setting-text">
                      <div class="about-name">
                        Argus<span class="about-version">{appInfo()?.version}</span>
                      </div>
                      <div class="setting-desc">快速、现代、信息丰富的 Windows 任务管理器。</div>
                      <div class="setting-desc">
                        {appInfo()?.elevated
                          ? "正在以管理员身份运行。"
                          : "正在以标准权限运行，部分系统进程的信息与操作受限。"}
                      </div>
                      <div class="setting-desc">© 2026 Argus contributors</div>
                    </div>
                    <div class="setting-control">
                      <button type="button" class="btn" onClick={() => api.quit()}>
                        退出 Argus
                      </button>
                    </div>
                  </div>
                  <Setting
                    icon={<DocumentIcon size={20} />}
                    title="许可证"
                    desc="Argus 是自由软件，以 GNU Affero General Public License 第 3 版或（由你选择）任何更高版本发布，你可以按这一许可证的条款再分发和修改它。Argus 不提供任何担保。"
                  >
                    <button type="button" class="btn" onClick={() => setLicenseOpen(true)}>
                      查看许可证
                    </button>
                  </Setting>
                </section>
              </>
            )}
          </Show>
        </div>
      </div>
      <Portal>
        <LicenseDialog />
      </Portal>
    </div>
  );
}
