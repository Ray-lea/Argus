// First-run onboarding: the settings that matter on day one (language, tray,
// updates, the privileged service) and a few tips. Every step can be skipped;
// whatever is changed here is saved at once, like on the settings page, and
// nothing sends the user elsewhere before the end.

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
import { shortcutKeys } from "../lib/keys";
import { changeLanguage } from "../lib/language";
import { EASE_OUT, play, presence } from "../lib/motion";
import { appInfo } from "../lib/store";
import type { PrivilegedServiceStatus, SettingsView, UpdateChannel, UpdateRoute } from "../lib/types";
import { toast } from "../lib/ui";
import { updateStatus } from "../lib/updates";
import Indicator from "./Indicator";
import { CheckIcon, GlobeIcon, ShieldIcon, SparkleIcon, TrayIcon, UpdateIcon, WarningIcon } from "./icons";
import { serviceState } from "./Settings";

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

const STEPS = ["welcome", "background", "update", "privilege", "tips"] as const;
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
  const [settings, { mutate }] = createResource(() => api.settings().catch(() => null));
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
  const [service, { refetch: refetchService, mutate: mutateService }] = createResource(() =>
    api.privilegedServiceStatus().catch(() => null),
  );
  const [serviceBusy, setServiceBusy] = createSignal(false);
  const [serviceError, setServiceError] = createSignal<string | null>(null);
  const manageService = async (action: "install" | "start") => {
    if (serviceBusy()) return;
    setServiceBusy(true);
    setServiceError(null);
    try {
      mutateService(await api.managePrivilegedService(action));
    } catch (e) {
      setServiceError(String(e));
      void refetchService();
    } finally {
      setServiceBusy(false);
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
        <label class="onboarding-select">
          <GlobeIcon />
          <select
            class="select"
            aria-label={t("onboarding.welcome.language")}
            value={props.settings?.language ?? "system"}
            disabled={!props.settings}
            onChange={(e) => void choose(e.currentTarget.value as Language)}
          >
            <option value="system">{t("common.languageSystem")}</option>
            <For each={LOCALES}>{(l) => <option value={l.locale}>{l.name}</option>}</For>
          </select>
        </label>
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
                      <span class="seg-label">{t(`onboarding.update.${channel}`)}</span>
                    </button>
                  )}
                </For>
              </div>
            </Row>
            <Row title={t("onboarding.update.route")}>
              <select
                class="select"
                aria-label={t("onboarding.update.route")}
                value={s().updateRoute}
                onChange={(e) => void update(s(), { route: e.currentTarget.value as UpdateRoute })}
              >
                <option value="auto">{t("onboarding.update.routeAuto")}</option>
                <option value="direct">{t("onboarding.update.routeDirect")}</option>
                <option value="proxy">{t("onboarding.update.routeProxy")}</option>
              </select>
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

const FEATURES = ["end", "tune", "inspect", "control"] as const;

function Privilege(props: {
  status: PrivilegedServiceStatus | null | undefined;
  checking: boolean;
  busy: boolean;
  error: string | null;
  onManage: (action: "install" | "start") => void;
}) {
  const connected = () => props.status?.state === "running" && props.status.authorized && !props.status.error;
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
      <Show
        when={!appInfo()?.elevated}
        fallback={<p class="onboarding-note">{t("onboarding.privilege.elevated")}</p>}
      >
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
