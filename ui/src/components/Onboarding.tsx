// First-run onboarding: the settings that matter on day one (language, the
// navigation, animations and refresh rate, tray, updates, the privileged
// service and kernel sensors) and a few tips. Every step can be skipped;
// changes are saved as on the settings page, and nothing sends the user
// elsewhere before the end.

import {
  createEffect,
  createResource,
  createSignal,
  For,
  type JSX,
  Match,
  on,
  onCleanup,
  Show,
  Switch,
} from "solid-js";
import { type Language, LOCALES, rich, t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { changeKernelSensors, kernelBusy, kernelError, kernelRevision } from "../lib/kernel-setup";
import { shortcutKeys } from "../lib/keys";
import { changeLanguage } from "../lib/language";
import {
  type ChartMode,
  chartMode,
  EASE_OUT,
  play,
  presence,
  setChartMode,
  setUiMotion,
  uiMotion,
} from "../lib/motion";
import { noteServiceStatus, serviceConnected } from "../lib/privilege";
import { appInfo, changeInterval, INTERVALS, intervalMs } from "../lib/store";
import type {
  PrivilegedServiceStatus,
  SettingsView,
  TaskManagerStatus,
  UpdateChannel,
  UpdateRoute,
} from "../lib/types";
import { toast } from "../lib/ui";
import { updateStatus } from "../lib/updates";
import Indicator from "./Indicator";
import {
  BoltIcon,
  CheckIcon,
  GlobeIcon,
  KeyboardIcon,
  ShieldIcon,
  SparkleIcon,
  ThemeIcon,
  TrayIcon,
  UpdateIcon,
  WarningIcon,
} from "./icons";
import { NavPositionChoice } from "./Nav";
import Select from "./Select";
import { chartModeOptions, serviceState } from "./Settings";

/**
 * The onboarding a start shows if the settings record an older one. Raise it
 * when a step is added that existing users should see too.
 */
export const ONBOARDING_VERSION = 1;

const [open, setOpen] = createSignal(false);
export const onboardingOpen = open;

/** Opens the onboarding again (from the about section). */
export const openOnboarding = () => setOpen(true);

/** Shows the onboarding on the first start of this version of it. */
export async function maybeOpenOnboarding() {
  const settings = await api.settings().catch(() => null);
  if (settings && settings.onboarding < ONBOARDING_VERSION) setOpen(true);
}

const STEPS = [
  "welcome",
  "display",
  "background",
  "update",
  "privilege",
  "sensors",
  "taskManager",
  "tips",
] as const;
type Step = (typeof STEPS)[number];

export default function Onboarding() {
  const shown = presence(open, 170);
  return (
    <Show when={shown.value()}>
      <OnboardingDialog leaving={shown.leaving()} />
    </Show>
  );
}

function OnboardingDialog(props: { leaving: boolean }) {
  const [index, setIndex] = createSignal(0);
  const [settings, { mutate }] = createResource(kernelRevision, () => api.settings().catch(() => null));
  let stepEl: HTMLDivElement | undefined;
  let primary: HTMLButtonElement | undefined;

  const step = (): Step => STEPS[index()];
  const last = () => index() === STEPS.length - 1;

  const close = () => {
    if (props.leaving) return;
    setOpen(false);
    void api.completeOnboarding(ONBOARDING_VERSION).catch(() => {});
  };
  const go = (delta: number) => setIndex((i) => Math.min(STEPS.length - 1, Math.max(0, i + delta)));

  // Each step slides in from the side it comes from; focus follows the main button.
  let previous = 0;
  createEffect(
    on(index, (i) => {
      const dir = Math.sign(i - previous);
      previous = i;
      queueMicrotask(() => primary?.focus());
      if (dir === 0) return;
      play(
        stepEl,
        [
          { opacity: 0, transform: `translateX(${dir * 24}px)` },
          { opacity: 1, transform: "none" },
        ],
        { duration: 250, easing: EASE_OUT },
      );
    }),
  );

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));

  const save = async (change: Promise<SettingsView>) => {
    try {
      mutate(await change);
    } catch (e) {
      toast("error", t("onboarding.saveFailed", { error: String(e) }));
    }
  };

  // The service is installed right here; the state survives paging back and forth.
  const [service, { refetch: refetchService, mutate: mutateService }] = createResource(async () => {
    const status = await api.privilegedServiceStatus().catch(() => null);
    if (status) noteServiceStatus(status);
    return status;
  });
  const [serviceBusy, setServiceBusy] = createSignal(false);
  const [serviceError, setServiceError] = createSignal<string | null>(null);
  const manageService = async (action: "install" | "start") => {
    if (serviceBusy()) return;
    setServiceBusy(true);
    setServiceError(null);
    try {
      const status = await api.managePrivilegedService(action);
      mutateService(status);
      noteServiceStatus(status);
    } catch (e) {
      setServiceError(String(e));
      void refetchService();
    } finally {
      setServiceBusy(false);
    }
  };

  const enableSensors = async () => {
    if (kernelBusy() || serviceBusy() || service.loading || !serviceConnected(service())) return;
    const value = await changeKernelSensors(true);
    if (value) mutate(value);
    else void refetchService();
  };

  // The replacement needs the service; its state follows the service's.
  const [taskManager, { refetch: refetchTaskManager, mutate: mutateTaskManager }] = createResource(
    service,
    () => api.taskManagerStatus().catch(() => null),
  );
  const [taskManagerBusy, setTaskManagerBusy] = createSignal(false);
  const [taskManagerError, setTaskManagerError] = createSignal<string | null>(null);
  const enableTaskManager = async () => {
    if (taskManagerBusy()) return;
    setTaskManagerBusy(true);
    setTaskManagerError(null);
    try {
      mutateTaskManager(await api.manageTaskManager("enable"));
    } catch (e) {
      setTaskManagerError(String(e));
      void refetchTaskManager();
    } finally {
      setTaskManagerBusy(false);
    }
  };

  return (
    <div class="scrim onboarding-scrim" classList={{ leaving: props.leaving }}>
      <div class="dialog onboarding" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
        <div class="onboarding-step" ref={stepEl}>
          <Switch>
            <Match when={step() === "welcome"}>
              <Welcome settings={settings()} onChange={(view) => mutate(view)} />
            </Match>
            <Match when={step() === "display"}>
              <Display />
            </Match>
            <Match when={step() === "background"}>
              <Background settings={settings()} save={save} />
            </Match>
            <Match when={step() === "update"}>
              <Updates settings={settings()} save={save} />
            </Match>
            <Match when={step() === "privilege"}>
              <Privilege
                status={service()}
                checking={service.loading}
                busy={serviceBusy()}
                error={serviceError()}
                onManage={(action) => void manageService(action)}
              />
            </Match>
            <Match when={step() === "taskManager"}>
              <TaskManager
                status={taskManager()}
                serviceReady={serviceConnected(service())}
                checking={taskManager.loading || service.loading}
                busy={taskManagerBusy()}
                error={taskManagerError()}
                onEnable={() => void enableTaskManager()}
              />
            </Match>
            <Match when={step() === "sensors"}>
              <KernelSensors
                enabled={settings()?.kernelSensors ?? false}
                serviceReady={serviceConnected(service())}
                checking={settings.loading || service.loading || serviceBusy()}
                onEnable={() => void enableSensors()}
                onService={() => setIndex(STEPS.indexOf("privilege"))}
              />
            </Match>
            <Match when={step() === "tips"}>
              <Tips />
            </Match>
          </Switch>
        </div>
        <div class="dialog-footer onboarding-footer">
          <div
            class="onboarding-dots"
            role="img"
            aria-label={t("onboarding.step", { current: index() + 1, total: STEPS.length })}
          >
            <For each={STEPS}>{(_, i) => <span classList={{ current: i() === index() }} />}</For>
          </div>
          <Show when={!last()}>
            <button type="button" class="btn ghost" onClick={close}>
              {t("onboarding.skip")}
            </button>
          </Show>
          <Show when={index() > 0}>
            <button type="button" class="btn" onClick={() => go(-1)}>
              {t("onboarding.back")}
            </button>
          </Show>
          <button type="button" class="btn accent" ref={primary} onClick={() => (last() ? close() : go(1))}>
            {last() ? t("onboarding.finish") : t("onboarding.next")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** A step's heading: icon, title and introduction. */
function Heading(props: { icon: JSX.Element; title: string; body: string }) {
  return (
    <header class="onboarding-heading">
      <span class="onboarding-icon">{props.icon}</span>
      <h2 id="onboarding-title" class="dialog-title">
        {props.title}
      </h2>
      <p class="dialog-text">{props.body}</p>
    </header>
  );
}

/** A labelled control, laid out like a setting. */
function Row(props: { title: string; hint?: string; children: JSX.Element }) {
  return (
    <div class="onboarding-row">
      <div class="onboarding-row-text">
        <div>{props.title}</div>
        <Show when={props.hint}>
          <div class="onboarding-hint">{props.hint}</div>
        </Show>
      </div>
      {props.children}
    </div>
  );
}

function Toggle(props: { checked: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      onClick={() => props.onChange(!props.checked)}
    />
  );
}

function Welcome(props: {
  settings: SettingsView | null | undefined;
  onChange: (view: SettingsView) => void;
}) {
  const choose = async (language: Language) => {
    try {
      props.onChange(await changeLanguage(language));
    } catch (e) {
      toast("error", t("onboarding.saveFailed", { error: String(e) }));
    }
  };
  return (
    <>
      <div class="onboarding-hero">
        <img src="/logo.svg" alt="" width="72" height="72" />
      </div>
      <h2 id="onboarding-title" class="dialog-title onboarding-welcome-title">
        {t("onboarding.welcome.title")}
      </h2>
      <p class="dialog-text">{t("onboarding.welcome.body")}</p>
      <Row title={t("onboarding.welcome.language")}>
        <div class="onboarding-select">
          <GlobeIcon />
          <Select<Language>
            label={t("onboarding.welcome.language")}
            value={props.settings?.language ?? "system"}
            disabled={!props.settings}
            options={[
              { value: "system", label: t("common.languageSystem") },
              ...LOCALES.map((l) => ({ value: l.locale, label: l.name })),
            ]}
            onChange={(language) => void choose(language)}
          />
        </div>
      </Row>
    </>
  );
}

function Display() {
  return (
    <>
      <Heading
        icon={<ThemeIcon size={22} />}
        title={t("onboarding.display.title")}
        body={t("onboarding.display.body")}
      />
      <Row title={t("onboarding.display.nav")} hint={t("onboarding.display.navHint")}>
        <NavPositionChoice label={t("onboarding.display.nav")} />
      </Row>
      <Row title={t("onboarding.display.motion")} hint={t("onboarding.display.motionHint")}>
        <Toggle checked={uiMotion()} label={t("onboarding.display.motion")} onChange={setUiMotion} />
      </Row>
      <Row title={t("onboarding.display.chart")} hint={t("onboarding.display.chartHint")}>
        <Select<ChartMode>
          label={t("onboarding.display.chart")}
          value={chartMode()}
          options={chartModeOptions()}
          onChange={setChartMode}
        />
      </Row>
      <Row title={t("onboarding.display.interval")} hint={t("onboarding.display.intervalHint")}>
        <Select
          class="tnum"
          label={t("onboarding.display.interval")}
          value={intervalMs()}
          options={INTERVALS.map((ms) => ({
            value: ms,
            label: t("shell.interval.option", { seconds: ms / 1000 }),
          }))}
          onChange={changeInterval}
        />
      </Row>
    </>
  );
}

function Background(props: {
  settings: SettingsView | null | undefined;
  save: (change: Promise<SettingsView>) => Promise<void>;
}) {
  const keys = () => shortcutKeys(props.settings?.hotkey ?? "");
  return (
    <>
      <Heading
        icon={<TrayIcon size={22} />}
        title={t("onboarding.background.title")}
        body={t("onboarding.background.body")}
      />
      <Show when={props.settings}>
        {(s) => (
          <>
            <Row title={t("onboarding.background.autostart")} hint={t("onboarding.background.autostartHint")}>
              <Toggle
                checked={s().autostart}
                label={t("onboarding.background.autostart")}
                onChange={(on) => void props.save(api.setAutostart(on))}
              />
            </Row>
            <Row
              title={t("onboarding.background.closeToTray")}
              hint={t("onboarding.background.closeToTrayHint")}
            >
              <Toggle
                checked={s().closeToTray}
                label={t("onboarding.background.closeToTray")}
                onChange={(on) => void props.save(api.setCloseToTray(on))}
              />
            </Row>
            <p class="onboarding-note">
              <Show when={keys().length > 0} fallback={t("onboarding.background.hotkeyOff")}>
                {rich("onboarding.background.hotkey", { hotkey: <Keycaps keys={keys()} /> })}
              </Show>
            </p>
            <Show when={s().hotkeyError}>
              {(error) => (
                <p class="onboarding-note warning">
                  <WarningIcon size={14} />
                  {error()}
                </p>
              )}
            </Show>
          </>
        )}
      </Show>
    </>
  );
}

function Keycaps(props: { keys: string[] }) {
  return (
    <span class="keycaps">
      <For each={props.keys}>{(key) => <kbd>{key}</kbd>}</For>
    </span>
  );
}

function Updates(props: {
  settings: SettingsView | null | undefined;
  save: (change: Promise<SettingsView>) => Promise<void>;
}) {
  const disabled = () => updateStatus()?.state === "disabled";
  const update = (
    s: SettingsView,
    patch: { check?: boolean; channel?: UpdateChannel; route?: UpdateRoute },
  ) =>
    props.save(
      api.setUpdateSettings({
        check: s.updateCheck,
        channel: s.updateChannel,
        route: s.updateRoute,
        proxy: s.updateProxy,
        ...patch,
      }),
    );
  return (
    <>
      <Heading
        icon={<UpdateIcon size={22} />}
        title={t("onboarding.update.title")}
        body={t("onboarding.update.body")}
      />
      <Show when={props.settings}>
        {(s) => (
          <>
            <Row title={t("onboarding.update.check")} hint={t("onboarding.update.checkHint")}>
              <Toggle
                checked={s().updateCheck}
                label={t("onboarding.update.check")}
                onChange={(on) => void update(s(), { check: on })}
              />
            </Row>
            <Row
              title={t("onboarding.update.channel")}
              hint={s().updateChannel === "beta" ? t("onboarding.update.betaHint") : undefined}
            >
              <div class="segmented" role="group" aria-label={t("onboarding.update.channel")}>
                <Indicator
                  selector="[aria-pressed='true']"
                  track={() => s().updateChannel}
                  class="seg-thumb"
                />
                <For each={["stable", "beta"] as const}>
                  {(channel) => (
                    <button
                      type="button"
                      aria-pressed={s().updateChannel === channel}
                      onClick={() => void update(s(), { channel })}
                    >
                      <span>{t(`onboarding.update.${channel}`)}</span>
                    </button>
                  )}
                </For>
              </div>
            </Row>
            <Row title={t("onboarding.update.route")}>
              <Select<UpdateRoute>
                label={t("onboarding.update.route")}
                value={s().updateRoute}
                options={[
                  { value: "auto", label: t("onboarding.update.routeAuto") },
                  { value: "direct", label: t("onboarding.update.routeDirect") },
                  { value: "proxy", label: t("onboarding.update.routeProxy") },
                ]}
                onChange={(route) => void update(s(), { route })}
              />
            </Row>
            <p class="onboarding-note">{t("onboarding.update.proxyNotice")}</p>
            <Show when={disabled()}>
              <p class="onboarding-note">{updateStatus()?.error ?? t("onboarding.update.unavailable")}</p>
            </Show>
          </>
        )}
      </Show>
    </>
  );
}

const FEATURES = ["end", "tune", "inspect", "control", "sensors"] as const;

function Privilege(props: {
  status: PrivilegedServiceStatus | null | undefined;
  checking: boolean;
  busy: boolean;
  error: string | null;
  onManage: (action: "install" | "start") => void;
}) {
  const connected = () => serviceConnected(props.status);
  // What can be done from here: install, or start a stopped service of this account.
  const action = (): "install" | "start" | null => {
    if (props.status?.state === "notInstalled") return "install";
    if (props.status?.state === "stopped" && props.status.authorized) return "start";
    return null;
  };
  const state = () => {
    if (props.busy) return t("settings.privilege.state.busy");
    if (props.checking) return t("settings.privilege.state.checking");
    return serviceState(props.status);
  };
  const hint = () => {
    if (connected()) return t("onboarding.privilege.replace");
    return action() || props.busy ? t("onboarding.privilege.later") : t("onboarding.privilege.manage");
  };
  return (
    <>
      <Heading
        icon={<ShieldIcon size={22} />}
        title={t("onboarding.privilege.title")}
        body={t("onboarding.privilege.body")}
      />
      <ul class="onboarding-list">
        <For each={FEATURES}>{(f) => <li>{t(`onboarding.privilege.features.${f}`)}</li>}</For>
      </ul>
      <p class="onboarding-note warning">
        <WarningIcon size={14} />
        {t("onboarding.privilege.risk")}
      </p>
      <Show when={appInfo()?.elevated}>
        <p class="onboarding-note">{t("onboarding.privilege.elevated")}</p>
      </Show>
      <div class="onboarding-row">
        <div class="onboarding-row-text">
          <div class="onboarding-state" aria-live="polite">
            <Show when={props.busy}>
              <span class="spinner" />
            </Show>
            <Show when={connected() && !props.busy}>
              <CheckIcon size={14} />
            </Show>
            {state()}
          </div>
          <div class="onboarding-hint">{hint()}</div>
        </div>
        <Show when={action()}>
          {(a) => (
            <button
              type="button"
              class="btn"
              disabled={props.busy || props.checking}
              onClick={() => props.onManage(a())}
            >
              {a() === "install" ? t("settings.privilege.install.action") : t("settings.privilege.start")}
            </button>
          )}
        </Show>
      </div>
      <Show when={props.error}>
        {(error) => (
          <p class="onboarding-note warning">
            <WarningIcon size={14} />
            {error()}
          </p>
        )}
      </Show>
    </>
  );
}

function KernelSensors(props: {
  enabled: boolean;
  serviceReady: boolean;
  checking: boolean;
  onEnable: () => void;
  onService: () => void;
}) {
  const state = () => {
    if (kernelBusy()) return t("settings.sensors.preparing");
    if (props.checking) return t("onboarding.sensors.checking");
    return props.enabled ? t("onboarding.sensors.enabled") : t("onboarding.sensors.disabled");
  };
  return (
    <>
      <Heading
        icon={<BoltIcon size={22} />}
        title={t("onboarding.sensors.title")}
        body={t("onboarding.sensors.body")}
      />
      <p class="onboarding-note">{t("onboarding.sensors.install")}</p>
      <div class="onboarding-row">
        <div class="onboarding-row-text">
          <div class="onboarding-state" role="status">
            <Show when={kernelBusy()}>
              <span class="spinner" />
            </Show>
            <Show when={props.enabled && !kernelBusy()}>
              <CheckIcon size={14} />
            </Show>
            {state()}
          </div>
          <div class="onboarding-hint">
            {!props.serviceReady
              ? t("onboarding.sensors.needsService")
              : props.enabled
                ? t("onboarding.sensors.manage")
                : t("onboarding.sensors.later")}
          </div>
        </div>
        <Show
          when={!props.serviceReady}
          fallback={
            <Show when={!props.enabled}>
              <button
                type="button"
                class="btn"
                disabled={kernelBusy() || props.checking}
                onClick={props.onEnable}
              >
                {t("onboarding.sensors.enable")}
              </button>
            </Show>
          }
        >
          <button
            type="button"
            class="btn"
            disabled={kernelBusy() || props.checking}
            onClick={props.onService}
          >
            {t("onboarding.sensors.configureService")}
          </button>
        </Show>
      </div>
      <Show when={kernelError()}>
        {(error) => (
          <p class="onboarding-note warning" role="alert">
            <WarningIcon size={14} />
            {error()}
          </p>
        )}
      </Show>
    </>
  );
}

function TaskManager(props: {
  status: TaskManagerStatus | null | undefined;
  serviceReady: boolean;
  checking: boolean;
  busy: boolean;
  error: string | null;
  onEnable: () => void;
}) {
  const enabled = () => props.status?.state === "enabled";
  const state = () => {
    if (props.busy) return t("settings.taskManager.busy");
    if (props.checking) return t("settings.taskManager.checking");
    if (!props.serviceReady) return t("settings.privilege.state.notInstalled");
    const s = props.status;
    return s ? t(`settings.taskManager.state.${s.state}`) : t("settings.taskManager.unknown");
  };
  const hint = () => {
    if (!props.serviceReady) return t("onboarding.taskManager.needsService");
    if (enabled()) return props.status?.detail ?? "";
    if (props.status?.canEnable || props.busy) return t("onboarding.taskManager.later");
    return props.status?.detail ?? t("settings.taskManager.retry");
  };
  return (
    <>
      <Heading
        icon={<KeyboardIcon size={22} />}
        title={t("onboarding.taskManager.title")}
        body={t("onboarding.taskManager.body")}
      />
      <p class="onboarding-note">{t("onboarding.taskManager.scope")}</p>
      <p class="onboarding-note">{t("onboarding.taskManager.undo")}</p>
      <div class="onboarding-row">
        <div class="onboarding-row-text">
          <div class="onboarding-state" aria-live="polite">
            <Show when={props.busy}>
              <span class="spinner" />
            </Show>
            <Show when={enabled() && !props.busy}>
              <CheckIcon size={14} />
            </Show>
            {state()}
          </div>
          <div class="onboarding-hint">{hint()}</div>
        </div>
        <Show when={props.serviceReady && props.status?.canEnable}>
          <button
            type="button"
            class="btn"
            disabled={props.busy || props.checking}
            onClick={() => props.onEnable()}
          >
            {t("settings.taskManager.enable")}
          </button>
        </Show>
      </div>
      <Show when={props.error}>
        {(error) => (
          <p class="onboarding-note warning">
            <WarningIcon size={14} />
            {error()}
          </p>
        )}
      </Show>
    </>
  );
}

function Tips() {
  const kbd = (keys: string[]) => <Keycaps keys={keys} />;
  return (
    <>
      <Heading
        icon={<SparkleIcon size={22} />}
        title={t("onboarding.tips.title")}
        body={t("onboarding.tips.body")}
      />
      <ul class="onboarding-list">
        <li>
          {rich("onboarding.tips.pages", {
            keys: (
              <span class="keycaps">
                <kbd>Ctrl</kbd>
                <kbd>1</kbd>–<kbd>7</kbd>
              </span>
            ),
            search: kbd(["Ctrl", "F"]),
          })}
        </li>
        <li>{t("onboarding.tips.menu")}</li>
        <li>{t("onboarding.tips.drop")}</li>
        <li>{t("onboarding.tips.history")}</li>
      </ul>
      <p class="onboarding-note">{t("onboarding.tips.reopen")}</p>
    </>
  );
}
