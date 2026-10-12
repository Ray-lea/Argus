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
import type {
  PriorityClass,
  PrivilegedServiceAction,
  ProcessRow,
  ProcessRule,
  RuleStats,
  SettingsView,
  UpdateChannel,
  UpdateRoute,
} from "../lib/types";
import { confirm, toast } from "../lib/ui";
import { updatePending, updateStatus } from "../lib/updates";
import Indicator from "./Indicator";
import {
  BoltIcon,
  DismissIcon,
  DocumentIcon,
  ExternalIcon,
  GlobeIcon,
  HistoryIcon,
  InfoIcon,
  KeyboardIcon,
  PowerIcon,
  PulseIcon,
  ShieldIcon,
  SparkleIcon,
  StorageIcon,
  TimerIcon,
  TransferIcon,
  TrashIcon,
  TrayIcon,
  UpdateIcon,
} from "./icons";
import ProcIcon from "./ProcIcon";

const SECTIONS = [
  { id: "startup", label: "启动与后台", icon: PowerIcon },
  { id: "hotkey", label: "快捷键", icon: KeyboardIcon },
  { id: "privilege", label: "权限", icon: ShieldIcon },
  { id: "motion", label: "动画", icon: SparkleIcon },
  { id: "data", label: "数据与历史", icon: HistoryIcon },
  { id: "rules", label: "规则", icon: BoltIcon },
  { id: "update", label: "更新", icon: UpdateIcon },
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

const [privilegeRevision, setPrivilegeRevision] = createSignal(0);

function PrivilegedServiceSettings() {
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [status, { refetch, mutate }] = createResource(async () => {
    try {
      const value = await api.privilegedServiceStatus();
      setError(null);
      return value;
    } catch (e) {
      setError(String(e));
      return null;
    }
  });
  const manage = async (action: PrivilegedServiceAction) => {
    if (busy() || status.loading) return;
    const installing = action === "install";
    if (
      (installing || action === "uninstall") &&
      !(await confirm({
        title: installing ? "启用特权服务？" : "卸载特权服务？",
        body: installing
          ? "允许当前 Windows 账户通过后台服务管理系统进程、应用规则、统计每进程网络流量、查询文件占用、控制服务和断开连接。服务随 Windows 启动，此账户下的其他本机程序也能调用这些功能。安装需要管理员授权，以后可随时卸载。"
          : "先恢复 Windows 任务管理器，再停止并移除后台服务。恢复失败时停止卸载。Argus 继续运行，系统进程和服务的操作将重新受当前权限限制。卸载需要管理员授权。",
        confirmLabel: installing ? "安装并启用" : "卸载服务",
        danger: !installing,
      }))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      mutate(await api.managePrivilegedService(action));
      toast("success", action === "uninstall" ? "特权服务已卸载。" : "特权服务已就绪。");
    } catch (e) {
      await refetch();
      setError(String(e));
    } finally {
      setBusy(false);
      setPrivilegeRevision((value) => value + 1);
    }
  };
  const label = () => {
    if (busy()) return "正在等待管理员授权或处理服务…";
    if (status.loading) return "正在检查服务状态…";
    const s = status();
    if (!s) return "暂时无法读取状态";
    if (s.state === "notInstalled") return "未安装";
    if (s.state === "running")
      return s.authorized && !s.error ? `已连接 · ${s.version}` : "正在运行，但暂不可用";
    if (s.state === "stopped") return "已停止";
    if (s.state === "startPending") return "正在启动";
    if (s.state === "stopPending") return "正在停止";
    return "暂不可用";
  };
  // Updating Argus does not update the service, which needs administrator rights.
  const outdated = () => {
    const s = status();
    const app = appInfo()?.version;
    if (!s?.version || !app || s.version === app) return null;
    return `服务版本 ${s.version} 与 Argus ${app} 不一致，请单击“更新服务”。`;
  };
  return (
    <>
      <Setting
        icon={<ShieldIcon size={20} />}
        title="特权服务"
        desc="让标准权限下的 Argus 管理系统进程、自动应用规则、采集每进程网络流量、查询文件占用，以及控制服务和断开 IPv4 TCP 连接。安装和卸载时需要管理员授权。"
      />
      <Setting
        icon={<PulseIcon size={20} />}
        title={<span aria-live="polite">{label()}</span>}
        desc="Argus 运行时会自动连接。网络采集不可用时隐藏网络列；规则下方显示应用结果。"
        error={error() ?? status()?.error ?? outdated()}
      >
        <button type="button" class="btn" disabled={busy() || status.loading} onClick={() => void refetch()}>
          刷新状态
        </button>
        <Show when={status()}>
          {(s) => (
            <Show
              when={s().state !== "notInstalled"}
              fallback={
                <button type="button" class="btn" disabled={busy()} onClick={() => void manage("install")}>
                  安装并启用
                </button>
              }
            >
              <Show when={s().authorized && s().state === "stopped"}>
                <button
                  type="button"
                  class="btn"
                  disabled={busy() || status.loading}
                  onClick={() => void manage("start")}
                >
                  启动服务
                </button>
              </Show>
              <Show when={s().authorized}>
                <button
                  type="button"
                  class="btn"
                  disabled={busy() || status.loading}
                  onClick={() => void manage("update")}
                >
                  更新服务
                </button>
              </Show>
              <button
                type="button"
                class="btn"
                disabled={busy() || status.loading}
                onClick={() => void manage("uninstall")}
              >
                卸载
              </button>
            </Show>
          )}
        </Show>
      </Setting>
    </>
  );
}

function TaskManagerSettings() {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [status, { refetch, mutate }] = createResource(privilegeRevision, async () => {
    try {
      const value = await api.taskManagerStatus();
      setError(null);
      return value;
    } catch (e) {
      setError(String(e));
      return null;
    }
  });
  const manage = async (action: "enable" | "restore") => {
    if (busy() || status.loading) return;
    if (
      action === "enable" &&
      !(await confirm({
        title: "替换系统任务管理器？",
        body: "Ctrl+Shift+Esc 等任务管理器入口将打开 Argus。此更改作用于整台电脑，其他账户仍打开 Windows 任务管理器。开启需要管理员授权；程序启动失败时尝试打开原生任务管理器。请保留独立恢复程序，以便界面或服务无法运行时恢复。",
        confirmLabel: "启用替换",
      }))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      mutate(await api.manageTaskManager(action));
      toast("success", action === "restore" ? "已恢复 Windows 任务管理器。" : "任务管理器替换已启用。");
    } catch (e) {
      await refetch();
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const label = () => {
    if (busy()) return "正在处理任务管理器设置…";
    if (status.loading) return "正在检查任务管理器设置…";
    const s = status();
    if (!s) return "无法读取替换状态";
    return {
      disabled: "使用 Windows 任务管理器",
      enabled: "已替换为 Argus",
      recoveryNeeded: "需要恢复",
      conflict: "已有其他配置",
    }[s.state];
  };
  return (
    <>
      <Setting
        icon={<KeyboardIcon size={20} />}
        title="替换系统任务管理器"
        desc="让 Ctrl+Shift+Esc 等入口打开 Argus。默认关闭；独立恢复程序不依赖界面或服务。更新或卸载特权服务会先关闭替换。"
      />
      <Setting
        icon={<ShieldIcon size={20} />}
        title={<span aria-live="polite">{label()}</span>}
        desc={status()?.detail ?? "请刷新状态后重试。"}
        error={error()}
      >
        <button type="button" class="btn" disabled={busy() || status.loading} onClick={() => void refetch()}>
          刷新状态
        </button>
        <Show when={status()?.canEnable}>
          <button
            type="button"
            class="btn"
            disabled={busy() || status.loading}
            onClick={() => void manage("enable")}
          >
            启用替换
          </button>
        </Show>
        <Show when={status()?.canRestore}>
          <button
            type="button"
            class="btn"
            disabled={busy() || status.loading}
            onClick={() => void manage("restore")}
          >
            恢复 Windows 任务管理器
          </button>
        </Show>
      </Setting>
      <Show when={status()?.canRestore}>
        <p class="note">界面无法打开时，可运行独立恢复程序：{status()?.recoveryPath}</p>
      </Show>
    </>
  );
}

function ruleSummary(rule: ProcessRule, stats: RuleStats | undefined): string {
  if (rule.priority === null && rule.efficiency === null) return "选择要应用的优先级或效率模式。";
  if (!rule.enabled) return "已停用。";
  if (!stats || (stats.applied === 0 && stats.denied === 0 && stats.failed === 0))
    return "还没有遇到这个程序。";
  const parts = [];
  if (stats.applied > 0) parts.push(`已应用于 ${stats.applied} 个进程`);
  if (stats.denied > 0) parts.push(`${stats.denied} 个权限不足，可启用特权服务；受保护的进程仍可能受限`);
  if (stats.failed > 0) parts.push(`${stats.failed} 个未能应用，请检查特权服务状态后重新保存规则`);
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

/** Offered by default; the same list as `PROXY_PRESETS` in updater.rs. */
const PROXY_PRESETS = ["https://gh-proxy.com/", "https://gh-proxy.org/", "https://ghfast.top/"];

const hostOf = (url: string) => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

type UpdatePrefs = { check: boolean; channel: UpdateChannel; route: UpdateRoute; proxy: string };

/** The update status and what to do about it, then channel and route. */
function UpdateSettings(props: {
  settings: SettingsView;
  onChange: (change: Promise<SettingsView>) => Promise<boolean>;
}) {
  const [installing, setInstalling] = createSignal(false);
  // The custom proxy being typed; null while a preset is chosen.
  const [draft, setDraft] = createSignal<string | null>(
    PROXY_PRESETS.includes(props.settings.updateProxy) ? null : props.settings.updateProxy,
  );
  const status = () => updateStatus();
  const state = () => status()?.state;
  const busy = () => state() === "checking" || state() === "downloading" || state() === "installing";
  const save = (patch: Partial<UpdatePrefs>) =>
    props.onChange(
      api.setUpdateSettings({
        check: props.settings.updateCheck,
        channel: props.settings.updateChannel,
        route: props.settings.updateRoute,
        proxy: props.settings.updateProxy,
        ...patch,
      }),
    );
  const install = async () => {
    setInstalling(true);
    try {
      await api.installUpdate();
    } catch (e) {
      toast("error", `无法安装更新：${String(e)}`);
    } finally {
      setInstalling(false);
    }
  };

  const title = () => {
    const s = status();
    if (!s) return "正在读取更新状态…";
    switch (s.state) {
      case "disabled":
        return "不检查更新";
      case "idle":
        return "尚未检查更新";
      case "checking":
        return "正在检查更新…";
      case "upToDate":
        return "已是最新版本";
      case "available":
        return `发现新版本 ${s.version}`;
      case "downloading":
        return `正在下载 ${s.version}${s.total ? ` · ${Math.floor((s.received / s.total) * 100)}%` : ""}`;
      case "ready":
        return `${s.version} 已下载，可以安装`;
      case "installing":
        return `正在启动 ${s.version} 的安装程序…`;
      case "error":
        return "更新失败";
    }
  };
  const desc = () => {
    const s = status();
    if (!s) return "";
    switch (s.state) {
      case "disabled":
        return s.error;
      case "ready":
        return "安装时 Argus 会退出，安装程序只显示进度，完成后自动重新打开 Argus。";
      case "available":
        return "免安装版不能自动更新，请从发布页下载新版本。";
      case "downloading":
        return `已下载 ${fmtCache(s.received)}${s.total ? ` / ${fmtCache(s.total)}` : ""}，来自 ${s.source ?? "GitHub"}。`;
    }
    if (s.checkedAt)
      return `上次检查：${dateFmt.format(s.checkedAt)}${s.source ? `，来自 ${s.source}` : ""}。`;
    return "从 GitHub Releases 获取更新；安装包带有签名，校验通过才会安装。";
  };
  const showNotes = () =>
    Boolean(status()?.notes) && (state() === "available" || state() === "downloading" || state() === "ready");
  const proxyChoice = () => (draft() !== null ? "custom" : props.settings.updateProxy);

  return (
    <>
      <Setting
        icon={<UpdateIcon size={20} />}
        title={<span aria-live="polite">{title()}</span>}
        desc={desc()}
        error={state() === "error" || state() === "ready" ? status()?.error : null}
      >
        <Show when={state() === "ready"}>
          <button type="button" class="btn accent" disabled={installing()} onClick={install}>
            重启并更新
          </button>
        </Show>
        <Show when={state() === "available" && status()?.version}>
          {(version) => (
            <button type="button" class="btn" onClick={() => void api.openReleasePage(version())}>
              <ExternalIcon />
              前往下载
            </button>
          )}
        </Show>
        <Show when={state() !== "disabled" && state() !== "ready"}>
          <button
            type="button"
            class="btn"
            disabled={!status() || busy()}
            onClick={() => void api.checkUpdate()}
          >
            检查更新
          </button>
        </Show>
      </Setting>
      <Show when={showNotes()}>
        <Setting
          icon={<DocumentIcon size={20} />}
          title={`${status()?.version} 的更新内容`}
          desc={<pre class="update-notes">{status()?.notes}</pre>}
        />
      </Show>
      <Setting
        icon={<TimerIcon size={20} />}
        title="自动检查更新"
        desc="启动约 1 分钟后检查一次，之后每天一次。新版本在后台下载，由你决定何时安装。"
      >
        <ToggleSwitch
          label="自动检查更新"
          checked={props.settings.updateCheck}
          onChange={(check) => void save({ check })}
        />
      </Setting>
      <Setting
        icon={<SparkleIcon size={20} />}
        title="更新通道"
        desc="测试版会更早收到新功能，也可能不稳定。从测试版切回稳定版后，要等稳定版的版本号超过当前版本才会更新。"
      >
        <select
          class="select"
          aria-label="更新通道"
          value={props.settings.updateChannel}
          onChange={(e) => void save({ channel: e.currentTarget.value as UpdateChannel })}
        >
          <option value="stable">稳定版</option>
          <option value="beta">测试版</option>
        </select>
      </Setting>
      <Setting
        icon={<GlobeIcon size={20} />}
        title="下载线路"
        desc="加速代理由第三方提供，能看到你的 IP 地址和下载的文件。安装包的签名同时覆盖版本号，代理既不能篡改更新，也不能用旧版本冒充新版本。"
      >
        <select
          class="select"
          aria-label="下载线路"
          value={props.settings.updateRoute}
          onChange={(e) => void save({ route: e.currentTarget.value as UpdateRoute })}
        >
          <option value="auto">自动：先连 GitHub，失败时用加速代理</option>
          <option value="direct">只连 GitHub</option>
          <option value="proxy">只用加速代理</option>
        </select>
      </Setting>
      <Show when={props.settings.updateRoute !== "direct"}>
        <Setting
          icon={<TransferIcon size={20} />}
          title="加速代理"
          desc={
            props.settings.updateRoute === "auto"
              ? "GitHub 连不上时先试这个代理，再试其他内置代理。"
              : "所有更新请求都经过这个代理。"
          }
        >
          <select
            class="select"
            aria-label="加速代理"
            value={proxyChoice()}
            onChange={(e) => {
              const value = e.currentTarget.value;
              if (value === "custom") {
                setDraft(
                  PROXY_PRESETS.includes(props.settings.updateProxy)
                    ? "https://"
                    : props.settings.updateProxy,
                );
              } else {
                setDraft(null);
                void save({ proxy: value });
              }
            }}
          >
            <For each={PROXY_PRESETS}>{(p) => <option value={p}>{hostOf(p)}</option>}</For>
            <option value="custom">自定义…</option>
          </select>
        </Setting>
        <Show when={draft() !== null}>
          <Setting
            icon={<GlobeIcon size={20} />}
            title="自定义代理"
            desc="加在 GitHub 地址前面的前缀，例如 https://example.com/，必须使用 https。"
          >
            <form
              class="rule-add"
              onSubmit={async (e) => {
                e.preventDefault();
                const proxy = draft()?.trim() ?? "";
                if (await save({ proxy })) setDraft(PROXY_PRESETS.includes(proxy) ? null : proxy);
              }}
            >
              <input
                class="input"
                aria-label="自定义代理地址"
                spellcheck={false}
                value={draft() ?? ""}
                onInput={(e) => setDraft(e.currentTarget.value)}
              />
              <button
                type="submit"
                class="btn"
                disabled={!draft()?.trim() || draft()?.trim() === props.settings.updateProxy}
              >
                保存
              </button>
            </form>
          </Setting>
        </Show>
      </Show>
    </>
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
      return true;
    } catch (e) {
      toast("error", `${failure}：${String(e)}`);
      return false;
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
                <Show when={s.id === "update" && updatePending()}>
                  <span class="nav-dot" role="img" aria-label="有新版本" />
                </Show>
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

                <section id="settings-privilege" class="settings-section">
                  <h2>权限</h2>
                  <PrivilegedServiceSettings />
                  <TaskManagerSettings />
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

                <section id="settings-update" class="settings-section">
                  <h2>更新</h2>
                  <UpdateSettings settings={s()} onChange={(change) => apply(change, "无法保存更新设置")} />
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
