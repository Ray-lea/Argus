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
  on,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { copyText, PRIORITY_LABEL } from "../lib/actions";
import { fmtCache, netUnit, setNetUnit } from "../lib/format";
import {
  type ChordKey,
  type HotkeyIssue,
  type HotkeyWarning,
  heldModifier,
  judge,
  keyByCode,
  keyName,
  type RecordHint,
  type Recording,
  record,
  startRecording,
} from "../lib/hotkey";
import { type Language, LOCALES, localized, type MessageKey, t } from "../lib/i18n";
import { api, inTauri } from "../lib/ipc";
import { changeKernelSensors, kernelBusy, kernelError, kernelRevision } from "../lib/kernel-setup";
import { hotkeyErrorText, hotkeyIssueText, keyLabel, shortcutKeys, shortcutText } from "../lib/keys";
import { changeLanguage } from "../lib/language";
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
import { noteServiceStatus, privilege, serviceConnected } from "../lib/privilege";
import {
  appInfo,
  changeInterval,
  focusRule,
  INTERVALS,
  intervalMs,
  setFocusRule,
  setPage,
  snapshot,
} from "../lib/store";
import type {
  HistoryUsage,
  HotkeyError,
  PriorityClass,
  PrivilegedServiceAction,
  PrivilegedServiceStatus,
  ProcessRow,
  ProcessRule,
  RuleStats,
  SettingsView,
  Theme,
  UpdateChannel,
  UpdateRoute,
} from "../lib/types";
import { confirm, toast } from "../lib/ui";
import { updatePending, updateStatus } from "../lib/updates";
import Indicator from "./Indicator";
import {
  BoltIcon,
  ChatIcon,
  DismissIcon,
  DocumentIcon,
  ExternalIcon,
  GlobeIcon,
  HistoryIcon,
  InfoIcon,
  KeyboardIcon,
  NavSideIcon,
  NavTopIcon,
  PowerIcon,
  PulseIcon,
  ShieldIcon,
  SparkleIcon,
  StorageIcon,
  ThemeIcon,
  TimerIcon,
  TransferIcon,
  TrashIcon,
  TrayIcon,
  UpdateIcon,
} from "./icons";
import { NavPositionChoice, navPosition } from "./Nav";
import { onboardingOpen, openOnboarding } from "./Onboarding";
import ProcIcon from "./ProcIcon";
import Select from "./Select";

const SECTIONS = [
  { id: "language", icon: GlobeIcon },
  { id: "startup", icon: PowerIcon },
  { id: "hotkey", icon: KeyboardIcon },
  { id: "privilege", icon: ShieldIcon },
  { id: "motion", icon: SparkleIcon },
  { id: "data", icon: HistoryIcon },
  { id: "rules", icon: BoltIcon },
  { id: "update", icon: UpdateIcon },
  { id: "about", icon: InfoIcon },
] as const;
type SectionId = (typeof SECTIONS)[number]["id"];

/** A section to bring into view when the page opens. */
const [pendingSection, setPendingSection] = createSignal<SectionId | null>(null);

/** Opens the settings page at a section (from another page). */
export function openSettings(section: SectionId) {
  setPendingSection(section);
  setPage("settings");
}

/** The feedback group (Settings > About; the invitation link is in commands.rs). */
const QQ_GROUP = "453803497";
const THEMES: Theme[] = ["system", "light", "dark"];
const dateFmt = localized(
  (l) =>
    new Intl.DateTimeFormat(l, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }),
);

/** One setting: icon, what it is, and its control on the right. */
function Setting(props: {
  icon: JSX.Element;
  title: JSX.Element;
  desc?: JSX.Element;
  error?: string | null;
  /** Neutral information below the description. */
  note?: string | null;
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
        <Show when={props.note}>{(note) => <div class="setting-note">{note()}</div>}</Show>
      </div>
      <Show when={control()}>
        <div class="setting-control">{control()}</div>
      </Show>
    </div>
  );
}

type Tone = "ok" | "warn" | "off" | "busy";

/**
 * One feature in one card: what it is and its state on top; below a divider,
 * what it reports now (and any error) beside what can be done.
 */
function FeatureCard(props: {
  icon: JSX.Element;
  title: string;
  desc: string;
  state: string;
  tone: Tone;
  note: JSX.Element;
  error?: string | null;
  footer?: JSX.Element;
  children?: JSX.Element;
}) {
  return (
    <div class="feature">
      <div class="feature-head">
        <span class="setting-icon">{props.icon}</span>
        <div class="setting-text">
          <div class="setting-title">{props.title}</div>
          <div class="setting-desc">{props.desc}</div>
        </div>
        <span class="state-pill" data-tone={props.tone} aria-live="polite">
          <Show when={props.tone === "busy"} fallback={<span class="state-dot" />}>
            <span class="spinner" />
          </Show>
          {props.state}
        </span>
      </div>
      <div class="feature-body">
        <div class="feature-note">
          <div>{props.note}</div>
          <Show when={props.error}>{(error) => <div class="setting-error">{error()}</div>}</Show>
        </div>
        <div class="setting-control">{props.children}</div>
      </div>
      <Show when={props.footer}>
        <div class="feature-foot">{props.footer}</div>
      </Show>
    </div>
  );
}

function ToggleSwitch(props: {
  checked: boolean;
  label: string;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <>
      <span class="switch-label">{props.checked ? t("settings.toggle.on") : t("settings.toggle.off")}</span>
      <button
        type="button"
        role="switch"
        class="switch"
        aria-checked={props.checked}
        aria-label={props.label}
        disabled={props.disabled}
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
/** Bumped when the onboarding, which can install the service, closes. */
const [serviceRevision, setServiceRevision] = createSignal(0);

/** The chart modes, labelled with their frame rates (also used by the onboarding). */
export function chartModeOptions(): { value: ChartMode; label: string }[] {
  return [
    { value: "fluid", label: t("settings.motion.chart.fluid", { fps: refreshHz() }) },
    {
      value: "balanced",
      label: t("settings.motion.chart.balanced", { fps: Math.round(1000 / balancedFrameMs()) }),
    },
    { value: "pixel", label: t("settings.motion.chart.pixel") },
    { value: "off", label: t("settings.motion.chart.off") },
  ];
}

/** The privileged service's state in a few words (also used by the onboarding). */
export function serviceState(s: PrivilegedServiceStatus | null | undefined): string {
  if (!s) return t("settings.privilege.state.unknown");
  if (s.state === "notInstalled") return t("settings.privilege.state.notInstalled");
  if (s.state === "running")
    return s.authorized && !s.error
      ? t("settings.privilege.state.connected", { version: s.version ?? "" })
      : t("settings.privilege.state.unreachable");
  if (s.state === "stopped") return t("settings.privilege.state.stopped");
  if (s.state === "startPending") return t("settings.privilege.state.startPending");
  if (s.state === "stopPending") return t("settings.privilege.state.stopPending");
  return t("settings.privilege.state.unavailable");
}

function PrivilegedServiceSettings() {
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [status, { refetch, mutate }] = createResource(serviceRevision, async () => {
    try {
      const value = await api.privilegedServiceStatus();
      setError(null);
      noteServiceStatus(value);
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
        title: installing ? t("settings.privilege.install.title") : t("settings.privilege.uninstall.title"),
        body: installing ? t("settings.privilege.install.body") : t("settings.privilege.uninstall.body"),
        confirmLabel: installing
          ? t("settings.privilege.install.action")
          : t("settings.privilege.uninstall.confirm"),
        danger: !installing,
      }))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      const value = await api.managePrivilegedService(action);
      mutate(value);
      noteServiceStatus(value);
      toast(
        "success",
        action === "uninstall" ? t("settings.privilege.uninstalled") : t("settings.privilege.ready"),
      );
    } catch (e) {
      await refetch();
      setError(String(e));
    } finally {
      setBusy(false);
      setPrivilegeRevision((value) => value + 1);
    }
  };
  const label = () => {
    if (busy()) return t("settings.privilege.state.busy");
    if (status.loading) return t("settings.privilege.state.checking");
    return serviceState(status());
  };
  const tone = (): Tone => {
    const s = status();
    if (busy() || status.loading) return "busy";
    if (s?.state === "notInstalled") return "off";
    return serviceConnected(s) ? "ok" : "warn";
  };
  // Updating Argus does not update the service, which needs administrator rights.
  const outdated = () => {
    const s = status();
    const app = appInfo()?.version;
    if (!s?.version || !app || s.version === app) return null;
    return t("settings.privilege.outdated", { service: s.version, app });
  };
  return (
    <FeatureCard
      icon={<ShieldIcon size={20} />}
      title={t("common.privilegedService")}
      desc={t("settings.privilege.desc")}
      state={label()}
      tone={tone()}
      note={t("settings.privilege.statusDesc")}
      error={error() ?? status()?.error ?? outdated()}
    >
      <button type="button" class="btn" disabled={busy() || status.loading} onClick={() => void refetch()}>
        {t("settings.refreshStatus")}
      </button>
      <Show when={status()}>
        {(s) => (
          <Show
            when={s().state !== "notInstalled"}
            fallback={
              <button type="button" class="btn" disabled={busy()} onClick={() => void manage("install")}>
                {t("settings.privilege.install.action")}
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
                {t("settings.privilege.start")}
              </button>
            </Show>
            <Show when={s().authorized}>
              <button
                type="button"
                class="btn"
                disabled={busy() || status.loading}
                onClick={() => void manage("update")}
              >
                {t("settings.privilege.update")}
              </button>
            </Show>
            <button
              type="button"
              class="btn"
              disabled={busy() || status.loading}
              onClick={() => void manage("uninstall")}
            >
              {t("settings.privilege.uninstall.action")}
            </button>
          </Show>
        )}
      </Show>
    </FeatureCard>
  );
}

function KernelSensorSettings(props: { enabled: boolean; onSaved: (value: SettingsView) => void }) {
  const toggle = async (enabled: boolean) => {
    const value = await changeKernelSensors(enabled);
    if (value) props.onSaved(value);
  };
  return (
    <Setting
      icon={<BoltIcon size={20} />}
      title={t("settings.sensors.title")}
      desc={
        <>
          {t("settings.sensors.desc")}
          <br />
          <span role="status">
            {kernelBusy()
              ? t("settings.sensors.preparing")
              : props.enabled
                ? t(`performance.cpu.sensorState.${snapshot()?.hardware.cpuSensors?.state ?? "pending"}`)
                : t("settings.sensors.off")}
          </span>
        </>
      }
      error={kernelError()}
    >
      <button
        type="button"
        class="btn"
        disabled={kernelBusy()}
        onClick={() => void api.openSensorDriverPage()}
      >
        <ExternalIcon />
        {t("settings.sensors.driver")}
      </button>
      <ToggleSwitch
        label={t("settings.sensors.title")}
        checked={props.enabled}
        disabled={kernelBusy()}
        onChange={(enabled) => void toggle(enabled)}
      />
    </Setting>
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
        title: t("settings.taskManager.confirm.title"),
        body: t("settings.taskManager.confirm.body"),
        confirmLabel: t("settings.taskManager.enable"),
      }))
    )
      return;
    setBusy(true);
    setError(null);
    try {
      mutate(await api.manageTaskManager(action));
      toast(
        "success",
        action === "restore" ? t("settings.taskManager.restored") : t("settings.taskManager.enabled"),
      );
    } catch (e) {
      await refetch();
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const label = () => {
    if (busy()) return t("settings.taskManager.busy");
    if (status.loading) return t("settings.taskManager.checking");
    const s = status();
    if (!s) return t("settings.taskManager.unknown");
    return t(`settings.taskManager.state.${s.state}`);
  };
  const tone = (): Tone => {
    const state = status()?.state;
    if (busy() || status.loading) return "busy";
    return state === "enabled" ? "ok" : state === "disabled" ? "off" : "warn";
  };
  return (
    <FeatureCard
      icon={<KeyboardIcon size={20} />}
      title={t("settings.taskManager.title")}
      desc={t("settings.taskManager.desc")}
      state={label()}
      tone={tone()}
      note={status()?.detail ?? t("settings.taskManager.retry")}
      error={error()}
      footer={
        status()?.canRestore ? (
          <span class="selectable">
            {t("settings.taskManager.recovery", { path: status()?.recoveryPath ?? "" })}
          </span>
        ) : undefined
      }
    >
      <button type="button" class="btn" disabled={busy() || status.loading} onClick={() => void refetch()}>
        {t("settings.refreshStatus")}
      </button>
      <Show when={status()?.canEnable}>
        <button
          type="button"
          class="btn"
          disabled={busy() || status.loading}
          onClick={() => void manage("enable")}
        >
          {t("settings.taskManager.enable")}
        </button>
      </Show>
      <Show when={status()?.canRestore}>
        <button
          type="button"
          class="btn"
          disabled={busy() || status.loading}
          onClick={() => void manage("restore")}
        >
          {t("settings.taskManager.restore")}
        </button>
      </Show>
    </FeatureCard>
  );
}

function ruleSummary(rule: ProcessRule, stats: RuleStats | undefined): string {
  if (rule.priority === null && rule.efficiency === null) return t("settings.rules.summary.noAction");
  if (!rule.enabled) return t("settings.rules.summary.disabled");
  if (!stats || (stats.applied === 0 && stats.denied === 0 && stats.failed === 0))
    return t("settings.rules.summary.notSeen");
  const parts = [];
  if (stats.applied > 0) parts.push(t("settings.rules.summary.applied", { count: stats.applied }));
  if (stats.denied > 0) parts.push(t("settings.rules.summary.denied", { count: stats.denied }));
  if (stats.failed > 0) parts.push(t("settings.rules.summary.failed", { count: stats.failed }));
  return t("settings.rules.summary.sentence", { parts: parts.join(t("settings.rules.summary.separator")) });
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
      setDraftError(t("settings.rules.exists", { name }));
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
        title={t("settings.rules.title")}
        desc={t("settings.rules.desc")}
        error={draftError()}
      >
        <form class="rule-add" onSubmit={add}>
          <input
            class="input"
            placeholder={t("settings.rules.placeholder")}
            aria-label={t("settings.rules.newName")}
            value={draft()}
            onInput={(e) => {
              setDraft(e.currentTarget.value);
              setDraftError(null);
            }}
          />
          <button type="submit" class="btn" disabled={!draft().trim()}>
            {t("settings.rules.add")}
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
              <Select
                label={t("settings.rules.priority.label", { name: rule.name })}
                value={rule.priority ?? ""}
                options={[
                  { value: "", label: t("settings.rules.priority.keep") },
                  ...PRIORITY_CHOICES.map((p) => ({
                    value: p,
                    label: t("settings.rules.priority.option", { priority: PRIORITY_LABEL[p] }),
                  })),
                ]}
                onChange={(v) => update(rule.id, { priority: (v || null) as PriorityClass | null })}
              />
              <Select
                label={t("settings.rules.efficiency.label", { name: rule.name })}
                value={rule.efficiency === null ? "" : rule.efficiency ? "on" : "off"}
                options={[
                  { value: "", label: t("settings.rules.efficiency.keep") },
                  { value: "on", label: t("settings.rules.efficiency.on") },
                  { value: "off", label: t("settings.rules.efficiency.off") },
                ]}
                onChange={(v) => update(rule.id, { efficiency: v === "" ? null : v === "on" })}
              />
              <button
                type="button"
                role="switch"
                class="switch"
                aria-checked={rule.enabled}
                aria-label={t("settings.rules.toggle", { name: rule.name })}
                title={rule.enabled ? t("settings.rules.disable") : t("settings.rules.enable")}
                onClick={() => update(rule.id, { enabled: !rule.enabled })}
              />
              <button
                type="button"
                class="btn ghost small icon"
                title={t("settings.rules.delete")}
                aria-label={t("settings.rules.deleteLabel", { name: rule.name })}
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

/** Long texts shown in a dialog, loaded when first opened. */
const DOCUMENTS = {
  license: {
    title: "GNU Affero General Public License",
    load: () => import("../../../LICENSE?raw").then((m) => m.default),
  },
  notices: {
    get title() {
      return t("settings.about.notices.dialog");
    },
    load: () => import("../../../THIRD-PARTY-NOTICES.txt?raw").then((m) => m.default),
  },
} as const;
type DocumentId = keyof typeof DOCUMENTS;

const [openDocument, setOpenDocument] = createSignal<DocumentId | null>(null);

function DocumentDialog() {
  // Keeps the last document while the dialog fades out.
  let last: DocumentId = "license";
  const shown = presence(() => openDocument() !== null, 170);
  const id = () => {
    const current = openDocument();
    if (current) last = current;
    return last;
  };
  return (
    <Show when={shown.value()}>
      <DocumentBody id={id()} leaving={shown.leaving()} />
    </Show>
  );
}

function DocumentBody(props: { id: DocumentId; leaving: boolean }) {
  const [text] = createResource(
    () => props.id,
    (id) => DOCUMENTS[id].load(),
  );
  const close = () => setOpenDocument(null);
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
            {DOCUMENTS[props.id].title}
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
            {t("common.close")}
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
      toast("error", t("settings.update.installError", { error: String(e) }));
    } finally {
      setInstalling(false);
    }
  };

  const title = () => {
    const s = status();
    if (!s) return t("settings.update.status.loading");
    const version = s.version ?? "";
    switch (s.state) {
      case "disabled":
        return t("settings.update.status.disabled");
      case "idle":
        return t("settings.update.status.idle");
      case "checking":
        return t("settings.update.status.checking");
      case "upToDate":
        return t("settings.update.status.upToDate");
      case "available":
        return t("settings.update.status.available", { version });
      case "downloading":
        return s.total
          ? t("settings.update.status.downloadingPercent", {
              version,
              percent: Math.floor((s.received / s.total) * 100),
            })
          : t("settings.update.status.downloading", { version });
      case "ready":
        return t("settings.update.status.ready", { version });
      case "installing":
        return t("settings.update.status.installing", { version });
      case "error":
        return t("settings.update.status.error");
    }
  };
  const desc = () => {
    const s = status();
    if (!s) return "";
    switch (s.state) {
      case "disabled":
        return s.error;
      case "ready":
        return t("settings.update.detail.ready");
      case "available":
        return t("settings.update.detail.portable");
      case "downloading": {
        const received = fmtCache(s.received);
        const source = s.source ?? "GitHub";
        return s.total
          ? t("settings.update.detail.downloadedOf", { received, total: fmtCache(s.total), source })
          : t("settings.update.detail.downloaded", { received, source });
      }
    }
    if (s.checkedAt) {
      const time = dateFmt().format(s.checkedAt);
      return s.source
        ? t("settings.update.detail.checkedFrom", { time, source: s.source })
        : t("settings.update.detail.checked", { time });
    }
    return t("settings.update.detail.source");
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
            {t("settings.update.restart")}
          </button>
        </Show>
        <Show when={state() === "available" && status()?.version}>
          {(version) => (
            <button type="button" class="btn" onClick={() => void api.openReleasePage(version())}>
              <ExternalIcon />
              {t("settings.update.download")}
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
            {t("settings.update.check")}
          </button>
        </Show>
      </Setting>
      <Show when={showNotes()}>
        <Setting
          icon={<DocumentIcon size={20} />}
          title={t("settings.update.notes", { version: status()?.version ?? "" })}
          desc={<pre class="update-notes">{status()?.notes}</pre>}
        />
      </Show>
      <Setting
        icon={<TimerIcon size={20} />}
        title={t("settings.update.auto.title")}
        desc={t("settings.update.auto.desc")}
      >
        <ToggleSwitch
          label={t("settings.update.auto.title")}
          checked={props.settings.updateCheck}
          onChange={(check) => void save({ check })}
        />
      </Setting>
      <Setting
        icon={<SparkleIcon size={20} />}
        title={t("settings.update.channel.label")}
        desc={t("settings.update.channel.desc")}
      >
        <Select<UpdateChannel>
          label={t("settings.update.channel.label")}
          value={props.settings.updateChannel}
          options={[
            { value: "stable", label: t("settings.update.channel.stable") },
            { value: "beta", label: t("settings.update.channel.beta") },
          ]}
          onChange={(channel) => void save({ channel })}
        />
      </Setting>
      <Setting
        icon={<GlobeIcon size={20} />}
        title={t("settings.update.route.label")}
        desc={t("settings.update.route.desc")}
      >
        <Select<UpdateRoute>
          label={t("settings.update.route.label")}
          value={props.settings.updateRoute}
          options={[
            { value: "auto", label: t("settings.update.route.auto") },
            { value: "direct", label: t("settings.update.route.direct") },
            { value: "proxy", label: t("settings.update.route.proxy") },
          ]}
          onChange={(route) => void save({ route })}
        />
      </Setting>
      <Show when={props.settings.updateRoute !== "direct"}>
        <Setting
          icon={<TransferIcon size={20} />}
          title={t("settings.update.proxy.label")}
          desc={
            props.settings.updateRoute === "auto"
              ? t("settings.update.proxy.fallback")
              : t("settings.update.proxy.only")
          }
        >
          <Select
            label={t("settings.update.proxy.label")}
            value={proxyChoice()}
            options={[
              ...PROXY_PRESETS.map((p) => ({ value: p, label: hostOf(p) })),
              { value: "custom", label: t("settings.update.proxy.custom") },
            ]}
            onChange={(value) => {
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
          />
        </Setting>
        <Show when={draft() !== null}>
          <Setting
            icon={<GlobeIcon size={20} />}
            title={t("settings.update.custom.title")}
            desc={t("settings.update.custom.desc")}
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
                aria-label={t("settings.update.custom.label")}
                spellcheck={false}
                value={draft() ?? ""}
                onInput={(e) => setDraft(e.currentTarget.value)}
              />
              <button
                type="submit"
                class="btn"
                disabled={!draft()?.trim() || draft()?.trim() === props.settings.updateProxy}
              >
                {t("settings.update.custom.save")}
              </button>
            </form>
          </Setting>
        </Show>
      </Show>
    </>
  );
}

/** The size of the saved history and its oldest record, once counted. */
function usageText(usage: HistoryUsage | undefined): string {
  if (!usage) return t("settings.data.history.counting");
  if (usage.bytes <= 0) return t("settings.data.history.empty");
  const size = fmtCache(usage.bytes);
  return usage.first
    ? t("settings.data.history.sizeSince", { size, date: dateFmt().format(usage.first) })
    : t("settings.data.history.size", { size });
}

/** Recording history, what it takes on disk, and clearing it. */
function HistorySettings(props: { enabled: boolean; onToggle: (on: boolean) => void }) {
  const [usage, { refetch }] = createResource(() => api.historyUsage());
  const [clearing, setClearing] = createSignal(false);

  const clear = async () => {
    const size = usage()?.bytes ?? 0;
    const ok = await confirm({
      title: t("settings.data.history.confirm.title"),
      body: t("settings.data.history.confirm.body", { size: fmtCache(size) }),
      confirmLabel: t("settings.data.history.clear"),
      danger: true,
    });
    if (!ok) return;
    setClearing(true);
    try {
      await api.clearHistory();
      toast("success", t("settings.data.history.cleared"), 2500);
    } catch (e) {
      toast("error", t("settings.data.history.clearError", { error: String(e) }));
    } finally {
      setClearing(false);
      void refetch();
    }
  };

  return (
    <>
      <Setting
        icon={<HistoryIcon size={20} />}
        title={t("settings.data.history.title")}
        desc={props.enabled ? t("settings.data.history.on") : t("settings.data.history.off")}
      >
        <ToggleSwitch
          label={t("settings.data.history.title")}
          checked={props.enabled}
          onChange={props.onToggle}
        />
      </Setting>
      <Setting
        icon={<StorageIcon size={20} />}
        title={t("settings.data.history.saved")}
        desc={usageText(usage())}
      >
        <button type="button" class="btn" disabled={clearing() || !usage()?.bytes} onClick={clear}>
          <TrashIcon />
          {t("settings.data.history.clear")}
        </button>
      </Setting>
    </>
  );
}

type HotkeyMode = "idle" | "recording" | "confirm" | "saving";

/** What the hotkey row says below its description. */
type HotkeyMessage =
  | { kind: "error"; error: unknown }
  | { kind: "issue"; issue: HotkeyIssue; hotkey: string }
  | { kind: "hint"; hint: RecordHint }
  | { kind: "maybeTaken" }
  | { kind: "warning"; warnings: HotkeyWarning[]; hotkey: string };

/** How a key of the chord being recorded reads, e.g. "左 Ctrl". */
function chordLabel(key: ChordKey): string {
  if (key.kind === "altGraph") return "AltGr";
  if (key.kind === "modifier") {
    const name = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", win: "Win" }[heldModifier(key.code) ?? "ctrl"];
    if (key.code.endsWith("Left")) return t("settings.hotkey.side.left", { key: name });
    if (key.code.endsWith("Right")) return t("settings.hotkey.side.right", { key: name });
    return name;
  }
  if (key.key) return keyLabel(key.key);
  const lock = keyByCode(key.code);
  return lock ? keyName(lock) : key.code;
}

/**
 * The show/hide hotkey: records a combination as RegisterHotKey will match it
 * (see `record` in lib/hotkey.ts) while the current one is suspended, so
 * that pressing it again reaches the page instead of hiding the window.
 */
function HotkeySetting(props: { settings: SettingsView; onSaved: (value: SettingsView) => void }) {
  const [mode, setMode] = createSignal<HotkeyMode>("idle");
  const [recording, setRecording] = createSignal<Recording>(startRecording());
  const [message, setMessage] = createSignal<HotkeyMessage | null>(null);
  /** The combination being confirmed or saved. */
  const [candidate, setCandidate] = createSignal("");
  let toggle!: HTMLButtonElement;
  let again: HTMLButtonElement | undefined;
  let endSession: (() => void) | null = null;
  let endKeys: (() => void) | null = null;
  let idleTimer = 0;
  let quietTimer = 0;

  const clearTimers = () => {
    clearTimeout(idleTimer);
    clearTimeout(quietTimer);
  };
  const detachKeys = () => {
    endKeys?.();
    endKeys = null;
    clearTimers();
  };
  /** Ends the recording; `resume` brings the old hotkey back. */
  const finish = (resume: boolean, keepMessage = false) => {
    detachKeys();
    endSession?.();
    endSession = null;
    setMode("idle");
    setRecording(startRecording());
    setCandidate("");
    if (!keepMessage) setMessage(null);
    if (resume) void api.resumeHotkey().then(props.onSaved, () => {});
    toggle?.focus();
  };
  const cancel = () => finish(true);

  // Ends the recording after a while without keys; the native side gives
  // the hotkey back on its own a little later in any case.
  const armIdle = () => {
    clearTimeout(idleTimer);
    idleTimer = window.setTimeout(cancel, 15_000);
  };
  // Modifiers held and nothing else arriving: the main key may be one that
  // Windows or another program takes before the page sees it.
  const armQuiet = (state: Recording) => {
    clearTimeout(quietTimer);
    const mods = state.held.map(heldModifier);
    if (state.pending || !mods.some((m) => m && m !== "shift") || mods.some((m) => !m)) return;
    quietTimer = window.setTimeout(() => {
      if (!message()) setMessage({ kind: "maybeTaken" });
    }, 1500);
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing) return;
    const before = recording();
    const { state, effect } = record(before, {
      type: e.type === "keydown" ? "down" : "up",
      code: e.code,
      key: e.key,
      keyCode: e.keyCode,
      ctrl: e.ctrlKey,
      alt: e.altKey,
      shift: e.shiftKey,
      meta: e.metaKey,
      altGraph: e.getModifierState("AltGraph"),
      repeat: e.repeat,
    });
    e.stopPropagation();
    // Tab still moves the focus as it ends the recording.
    if (!(effect.type === "cancel" && e.key === "Tab")) e.preventDefault();
    if (e.type === "keydown" && before.held.length === 0) setMessage(null);
    setRecording(state);
    armIdle();
    armQuiet(state);
    if (state.hint && state.hint !== before.hint) setMessage({ kind: "hint", hint: state.hint });
    if (effect.type === "cancel") cancel();
    else if (effect.type === "disable") void save("");
    else if (effect.type === "commit") attempt(effect.hotkey);
  };
  const attachKeys = () => {
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKey, true);
    endKeys = () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKey, true);
    };
    armIdle();
  };
  const listen = () => {
    setRecording(startRecording());
    setCandidate("");
    setMode("recording");
    attachKeys();
  };

  const begin = () => {
    setMessage(null);
    void api.suspendHotkey().catch(() => {});
    listen();
    const row = document.getElementById("setting-hotkey");
    const onBlur = () => cancel();
    const onHidden = () => {
      if (document.visibilityState === "hidden") cancel();
    };
    const onPointer = (e: PointerEvent) => {
      if (!row?.contains(e.target as Node)) cancel();
    };
    const onConfirmKey = (e: KeyboardEvent) => {
      if (mode() === "confirm" && e.key === "Escape") cancel();
    };
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onHidden);
    document.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onConfirmKey);
    endSession = () => {
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onHidden);
      document.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onConfirmKey);
    };
  };

  const attempt = (hotkey: string) => {
    const verdict = judge(hotkey);
    if (verdict.level === "block") {
      setMessage({ kind: "issue", issue: verdict.issue, hotkey });
      return;
    }
    if (verdict.level === "warn") {
      detachKeys();
      setCandidate(hotkey);
      setMessage({ kind: "warning", warnings: verdict.warnings, hotkey });
      setMode("confirm");
      // Thinking it over restarts the native side's wait as well; the
      // question goes away after as long as a recording would.
      void api.suspendHotkey().catch(() => {});
      armIdle();
      queueMicrotask(() => again?.focus());
      return;
    }
    void save(hotkey);
  };

  const save = async (hotkey: string) => {
    detachKeys();
    setCandidate(hotkey);
    setMode("saving");
    try {
      props.onSaved(await api.setHotkey(hotkey));
      finish(false);
    } catch (e) {
      setMessage({ kind: "error", error: e });
      const kind = (e as HotkeyError | null)?.kind;
      // Taken or refused: try another combination, the old one still suspended.
      if (hotkey && (kind === "taken" || kind === "failed" || kind === "invalid")) listen();
      else finish(true, true);
    }
  };

  onMount(() => {
    if (!inTauri) return;
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      // The native side brought the hotkey back itself (the window hid, or
      // the recording went on too long), or a retry succeeded.
      const unlisten = await listen("argus-hotkey-changed", async () => {
        if (mode() !== "idle") finish(false);
        props.onSaved(await api.settings());
      });
      onCleanup(unlisten);
    });
  });
  onCleanup(() => {
    if (mode() !== "idle") finish(true);
  });

  const shown = () => {
    if (mode() === "confirm" || mode() === "saving") return shortcutKeys(candidate());
    return [];
  };
  const errorText = () => {
    const m = message();
    if (!m) return hotkeyErrorText(props.settings.hotkeyError);
    if (m.kind === "error") return hotkeyErrorText(m.error);
    if (m.kind === "issue") return hotkeyIssueText(m.issue, m.hotkey);
    if (m.kind !== "hint") return null;
    switch (m.hint.kind) {
      case "noKey":
        return t("settings.hotkey.issue.noKey");
      case "order":
        return t("settings.hotkey.hint.order");
      case "altGraph":
        return t("settings.hotkey.hint.altGraph");
      case "unsupported": {
        const key =
          m.hint.key === "Unidentified" || !m.hint.key ? t("settings.hotkey.unknownKey") : m.hint.key;
        return t("settings.hotkey.issue.unsupported", { key });
      }
      default:
        return null;
    }
  };
  const noteText = () => {
    const m = message();
    if (m?.kind === "maybeTaken") return t("settings.hotkey.hint.maybeTaken");
    if (m?.kind === "hint" && m.hint.kind === "lockKey") return t("settings.hotkey.hint.lockKey");
    if (m?.kind === "warning") {
      const shortcut = shortcutText(m.hotkey);
      return m.warnings.map((w) => t(`settings.hotkey.warning.${w}`, { shortcut })).join(" ");
    }
    return null;
  };

  return (
    <Setting
      id="setting-hotkey"
      icon={<KeyboardIcon size={20} />}
      title={t("settings.hotkey.title")}
      desc={t("settings.hotkey.desc")}
      error={errorText()}
      note={noteText()}
    >
      <span class="keycaps" aria-live="polite">
        <Show
          when={mode() === "recording"}
          fallback={
            <Show when={mode() === "idle" ? shortcutKeys(props.settings.hotkey) : shown()} keyed>
              {(keys) => (
                <Show
                  when={keys.length > 0}
                  fallback={<span class="pending">{t("settings.hotkey.unset")}</span>}
                >
                  <For each={keys}>{(key) => <kbd>{key}</kbd>}</For>
                </Show>
              )}
            </Show>
          }
        >
          <Show
            when={recording().chord.length > 0}
            fallback={<span class="pending">{t("settings.hotkey.recording")}</span>}
          >
            <For each={recording().chord}>
              {(key) => <kbd data-ignored={key.kind === "lock" ? "" : undefined}>{chordLabel(key)}</kbd>}
            </For>
          </Show>
        </Show>
      </span>
      <Show when={mode() === "confirm"}>
        <button type="button" class="btn" onClick={() => void save(candidate())}>
          {t("settings.hotkey.confirmUse")}
        </button>
        <button
          ref={again}
          type="button"
          class="btn"
          onClick={() => {
            setMessage(null);
            void api.suspendHotkey().catch(() => {});
            listen();
          }}
        >
          {t("settings.hotkey.rerecord")}
        </button>
      </Show>
      <button
        ref={toggle}
        type="button"
        class="btn stacked"
        disabled={mode() === "saving"}
        onClick={() => (mode() === "idle" ? begin() : cancel())}
      >
        <span classList={{ off: mode() !== "idle" }}>{t("settings.hotkey.change")}</span>
        <span classList={{ off: mode() === "idle" }}>{t("common.cancel")}</span>
      </button>
    </Setting>
  );
}

export default function SettingsPage() {
  const [settings, { mutate, refetch }] = createResource(kernelRevision, () => api.settings());
  // The onboarding (opened from the about section) changes settings and may
  // install the service.
  createEffect(
    on(onboardingOpen, (open, wasOpen) => {
      if (!wasOpen || open) return;
      void refetch();
      setServiceRevision((value) => value + 1);
      setPrivilegeRevision((value) => value + 1);
    }),
  );
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

  /** Shows the new settings, or on failure the `failure` message with the error. */
  const apply = async (change: Promise<SettingsView>, failure: MessageKey) => {
    try {
      mutate(await change);
      return true;
    } catch (e) {
      toast("error", t(failure, { error: String(e) }));
      return false;
    }
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
        <nav class="settings-nav" aria-label={t("settings.nav")}>
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
                <span>{t(`settings.sections.${s.id}`)}</span>
                <Show when={s.id === "update" && updatePending()}>
                  <span class="nav-dot" role="img" aria-label={t("shell.updateAvailable")} />
                </Show>
              </button>
            )}
          </For>
        </nav>

        <div class="settings-content">
          <Show when={settings()}>
            {(s) => (
              <>
                <section id="settings-language" class="settings-section">
                  <h2>{t("settings.sections.language")}</h2>
                  <Setting
                    icon={<GlobeIcon size={20} />}
                    title={t("settings.language.title")}
                    desc={t("settings.language.desc")}
                  >
                    <Select<Language>
                      label={t("settings.language.title")}
                      value={s().language}
                      options={[
                        { value: "system", label: t("common.languageSystem") },
                        ...LOCALES.map((l) => ({ value: l.locale, label: l.name })),
                      ]}
                      onChange={(language) => apply(changeLanguage(language), "settings.language.error")}
                    />
                  </Setting>
                  <Setting
                    icon={<ThemeIcon size={20} />}
                    title={t("settings.theme.title")}
                    desc={t("settings.theme.desc")}
                  >
                    <div class="segmented" role="group" aria-label={t("settings.theme.title")}>
                      <Indicator selector="[aria-pressed='true']" track={() => s().theme} class="seg-thumb" />
                      <For each={THEMES}>
                        {(theme) => (
                          <button
                            type="button"
                            aria-pressed={s().theme === theme}
                            onClick={() => apply(api.setTheme(theme), "settings.theme.error")}
                          >
                            {t(`settings.theme.${theme}`)}
                          </button>
                        )}
                      </For>
                    </div>
                  </Setting>
                  <Setting
                    icon={navPosition() === "top" ? <NavTopIcon size={20} /> : <NavSideIcon size={20} />}
                    title={t("settings.navigation.title")}
                    desc={t("settings.navigation.desc")}
                  >
                    <NavPositionChoice label={t("settings.navigation.title")} />
                  </Setting>
                </section>

                <section id="settings-startup" class="settings-section">
                  <h2>{t("settings.sections.startup")}</h2>
                  <Setting
                    icon={<PowerIcon size={20} />}
                    title={t("settings.startup.autostart.title")}
                    desc={t("settings.startup.autostart.desc")}
                  >
                    <ToggleSwitch
                      label={t("settings.startup.autostart.title")}
                      checked={s().autostart}
                      onChange={(v) => apply(api.setAutostart(v), "settings.startup.autostart.error")}
                    />
                  </Setting>
                  <Setting
                    icon={<TrayIcon size={20} />}
                    title={t("settings.startup.closeToTray.title")}
                    desc={t("settings.startup.closeToTray.desc")}
                  >
                    <ToggleSwitch
                      label={t("settings.startup.closeToTray.title")}
                      checked={s().closeToTray}
                      onChange={(v) => apply(api.setCloseToTray(v), "settings.startup.closeToTray.error")}
                    />
                  </Setting>
                </section>

                <section id="settings-hotkey" class="settings-section">
                  <h2>{t("settings.sections.hotkey")}</h2>
                  <HotkeySetting settings={s()} onSaved={mutate} />
                </section>

                <section id="settings-privilege" class="settings-section">
                  <h2>{t("settings.sections.privilege")}</h2>
                  <PrivilegedServiceSettings />
                  <KernelSensorSettings enabled={s().kernelSensors} onSaved={mutate} />
                  <TaskManagerSettings />
                </section>

                <section id="settings-motion" class="settings-section">
                  <h2>{t("settings.sections.motion")}</h2>
                  <Setting
                    icon={<SparkleIcon size={20} />}
                    title={t("settings.motion.ui.title")}
                    desc={t("settings.motion.ui.desc")}
                  >
                    <ToggleSwitch
                      label={t("settings.motion.ui.title")}
                      checked={uiMotion()}
                      onChange={setUiMotion}
                    />
                  </Setting>
                  <Setting
                    icon={<PulseIcon size={20} />}
                    title={t("settings.motion.chart.title")}
                    desc={t("settings.motion.chart.desc")}
                  >
                    <Select<ChartMode>
                      label={t("settings.motion.chart.title")}
                      value={chartMode()}
                      options={chartModeOptions()}
                      onChange={setChartMode}
                    />
                  </Setting>
                </section>

                <section id="settings-data" class="settings-section">
                  <h2>{t("settings.sections.data")}</h2>
                  <Setting
                    icon={<TimerIcon size={20} />}
                    title={t("shell.interval.label")}
                    desc={t("settings.data.intervalDesc")}
                  >
                    <Select
                      class="tnum"
                      label={t("shell.interval.label")}
                      value={intervalMs()}
                      options={INTERVALS.map((ms) => ({
                        value: ms,
                        label: t("shell.interval.option", { seconds: ms / 1000 }),
                      }))}
                      onChange={changeInterval}
                    />
                  </Setting>
                  <Setting
                    icon={<TransferIcon size={20} />}
                    title={t("settings.data.netUnit.title")}
                    desc={t("settings.data.netUnit.desc")}
                  >
                    <div class="segmented" role="group" aria-label={t("settings.data.netUnit.title")}>
                      <Indicator selector="[aria-pressed='true']" track={netUnit} class="seg-thumb" />
                      <button
                        type="button"
                        title={t("settings.data.netUnit.bytes")}
                        aria-pressed={netUnit() === "bytes"}
                        onClick={() => setNetUnit("bytes")}
                      >
                        MB/s
                      </button>
                      <button
                        type="button"
                        title={t("settings.data.netUnit.bits")}
                        aria-pressed={netUnit() === "bits"}
                        onClick={() => setNetUnit("bits")}
                      >
                        Mbps
                      </button>
                    </div>
                  </Setting>
                  <HistorySettings
                    enabled={s().history}
                    onToggle={(on) => apply(api.setHistoryEnabled(on), "settings.data.history.error")}
                  />
                </section>

                <section id="settings-rules" class="settings-section">
                  <h2>{t("settings.sections.rules")}</h2>
                  <RulesSection
                    rules={s().rules}
                    onChange={(rules) => apply(api.setRules(rules), "settings.rules.error")}
                  />
                </section>

                <section id="settings-update" class="settings-section">
                  <h2>{t("settings.sections.update")}</h2>
                  <UpdateSettings
                    settings={s()}
                    onChange={(change) => apply(change, "settings.update.error")}
                  />
                </section>

                <section id="settings-about" class="settings-section">
                  <h2>{t("settings.sections.about")}</h2>
                  <div class="setting about">
                    <img class="about-logo" src="/logo.svg" alt="" width="48" height="48" />
                    <div class="setting-text">
                      <div class="about-name">
                        Argus<span class="about-version">{appInfo()?.version}</span>
                      </div>
                      <div class="setting-desc">{t("settings.about.tagline")}</div>
                      <div class="setting-desc">{t(`settings.about.${privilege()}`)}</div>
                      <div class="setting-desc">© 2026 Argus contributors</div>
                    </div>
                    <div class="setting-control">
                      <button type="button" class="btn" onClick={() => api.quit()}>
                        {t("settings.about.quit")}
                      </button>
                    </div>
                  </div>
                  <Setting
                    icon={<SparkleIcon size={20} />}
                    title={t("settings.about.onboarding.title")}
                    desc={t("settings.about.onboarding.desc")}
                  >
                    <button type="button" class="btn" onClick={openOnboarding}>
                      {t("settings.about.onboarding.open")}
                    </button>
                  </Setting>
                  <Setting
                    icon={<ChatIcon size={20} />}
                    title={t("settings.about.community.title")}
                    desc={t("settings.about.community.desc", { group: QQ_GROUP })}
                  >
                    <button
                      type="button"
                      class="btn"
                      onClick={() => void copyText(QQ_GROUP, t("settings.about.community.number"))}
                    >
                      {t("settings.about.community.copy")}
                    </button>
                    <button type="button" class="btn accent" onClick={() => void api.openCommunity()}>
                      {t("settings.about.community.join")}
                    </button>
                  </Setting>
                  <Setting
                    icon={<DocumentIcon size={20} />}
                    title={t("settings.about.license.title")}
                    desc={t("settings.about.license.desc")}
                  >
                    <button type="button" class="btn" onClick={() => setOpenDocument("license")}>
                      {t("settings.about.license.view")}
                    </button>
                  </Setting>
                  <Setting
                    icon={<DocumentIcon size={20} />}
                    title={t("settings.about.notices.title")}
                    desc={t("settings.about.notices.desc")}
                  >
                    <button type="button" class="btn" onClick={() => setOpenDocument("notices")}>
                      {t("settings.about.notices.view")}
                    </button>
                  </Setting>
                  <Setting
                    icon={<StorageIcon size={20} />}
                    title={t("settings.about.log.title")}
                    desc={t("settings.about.log.desc")}
                  >
                    <button type="button" class="btn" onClick={() => void api.openLogDir()}>
                      {t("settings.about.log.open")}
                    </button>
                  </Setting>
                </section>
              </>
            )}
          </Show>
        </div>
      </div>
      <Portal>
        <DocumentDialog />
      </Portal>
    </div>
  );
}
