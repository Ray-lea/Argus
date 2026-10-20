// The update dialog. For the update on offer: the version it brings beside
// this one, its size and signature, what changed since this version (signed
// changelog entries; skipped releases folded) and installing it. For the
// installed version: what changed, from CHANGELOG.md built into the app.
// Also says once per version that an update is ready, when the window is in
// view (never at start: Argus is often opened to deal with a busy system) and
// the onboarding is over.

import { createEffect, createResource, createSignal, on, onCleanup, Show, untrack } from "solid-js";
import { fmtCache } from "../lib/format";
import { t } from "../lib/i18n";
import { api, inTauri } from "../lib/ipc";
import { presence } from "../lib/motion";
import { changelogEntries, compareVersions, isPrerelease } from "../lib/release-notes";
import { appInfo } from "../lib/store";
import type { InstallError } from "../lib/types";
import { confirmRequest, toast } from "../lib/ui";
import {
  announced,
  installUpdate,
  openUpdateDialogAfter,
  setAnnounced,
  setUpdateDialog,
  shown,
  type UpdateDialogState,
  updateButtonVersion,
  updateDialog,
  updateDialogQueued,
  updateStatus,
} from "../lib/updates";
import { ExternalIcon, GlobeIcon, ShieldIcon, UpdateIcon } from "./icons";
import ProgressBar from "./ProgressBar";
import ReleaseNotes, { formatDate, UpdateVersions } from "./ReleaseNotes";

/** `busy`: the onboarding, which the notice and the dialog it opens wait for. */
export default function UpdateDialog(props: { busy: () => boolean }) {
  const shown = presence(updateDialog, 170);
  announceReady(props.busy);
  return (
    <Show when={shown.value()} keyed>
      {(state) => <Body state={state} leaving={shown.leaving()} />}
    </Show>
  );
}

/** Says once per version, when the window is in view and not `busy`, that an update waits. */
function announceReady(busy: () => boolean) {
  // Until a window started in the tray is first shown, the page may report
  // itself visible (see `shown`).
  const [onScreen, setOnScreen] = createSignal(!inTauri);
  if (inTauri) void shown().then(() => setOnScreen(true));
  createEffect(() => {
    const version = updateButtonVersion();
    if (!onScreen() || busy() || !version || version === announced()) return;
    const available = updateStatus()?.state === "available";
    const show = () => {
      if (document.visibilityState !== "visible") return false;
      setAnnounced(version);
      // Not beside the dialog itself, open or about to open.
      if (!untrack(updateDialog) && !updateDialogQueued())
        toast("info", t(available ? "updates.availableToast" : "updates.readyToast", { version }), 10000, {
          icon: () => <UpdateIcon />,
          actions: [{ label: t("updates.view"), run: () => openUpdateDialogAfter(busy) }],
        });
      return true;
    };
    if (show()) return;
    const onVisible = () => {
      if (show()) document.removeEventListener("visibilitychange", onVisible);
    };
    document.addEventListener("visibilitychange", onVisible);
    onCleanup(() => document.removeEventListener("visibilitychange", onVisible));
  });
}

function Body(props: { state: UpdateDialogState; leaving: boolean }) {
  const [starting, setStarting] = createSignal(false);
  const [failure, setFailure] = createSignal<InstallError | null>(null);
  const offer = props.state.mode === "offer";
  const from = props.state.mode === "installed" ? props.state.from : null;
  const current = () => appInfo()?.version ?? "";
  const state = () => (offer ? updateStatus()?.state : undefined);
  const target = () => (offer ? (updateStatus()?.version ?? "") : current());
  const busy = () => starting() || state() === "installing";

  // What an installed version changed comes with the app, loaded when asked for.
  const [changelog] = createResource(
    () => !offer || undefined,
    async () => changelogEntries((await import("../../../CHANGELOG.md?raw")).default),
  );
  const releases = () => (offer ? updateStatus()?.releases : changelog());
  const released = () => {
    const entry = releases()?.find((e) => compareVersions(e.version, target()) === 0);
    return entry ? formatDate(entry.date) : null;
  };

  const title = () => {
    const version = target();
    if (!offer) return t("updates.dialog.installed", { version });
    switch (state()) {
      case "available":
        return t("updates.dialog.available", { version });
      // Fetching the manifest again (a changed installer) before the version is known.
      case "checking":
      case "downloading":
        return version ? t("updates.dialog.downloading", { version }) : t("settings.update.status.checking");
      case "installing":
        return t("updates.dialog.installing");
      case "ready":
        return t("updates.dialog.ready", { version });
      // Nothing on offer any more while the dialog is open: say what happened.
      default:
        return t(`settings.update.status.${state() ?? "idle"}`);
    }
  };
  const subtitle = () =>
    [
      offer ? t(isPrerelease(target()) ? "updates.dialog.beta" : "updates.dialog.stable") : null,
      released() ? t("updates.dialog.released", { date: released() ?? "" }) : null,
    ]
      .filter(Boolean)
      .join(" · ");
  const before = () => (offer ? current() : from);
  const progress = () => {
    const s = updateStatus();
    return s?.total ? Math.min(1, s.received / s.total) : null;
  };

  const close = () => {
    if (!busy()) setUpdateDialog(null);
  };
  const install = async () => {
    setFailure(null);
    setStarting(true);
    const error = await installUpdate(appInfo()?.version);
    setStarting(false);
    if (error) setFailure(error);
  };
  // Why the installer did not start holds for the update it was about: it
  // stays while a changed installer is fetched again and after a declined
  // prompt, and goes once anything else settles (an update ready again, or a
  // new error, which then shows instead).
  createEffect(
    on(
      state,
      (now, before) => {
        const pending = now === "installing" || now === "checking" || now === "downloading";
        if (!pending && before !== "installing") setFailure(null);
      },
      { defer: true },
    ),
  );
  // Above the onboarding and the file lock dialog, it takes Escape for itself
  // (even while installing, when it stays open), as the confirmation does;
  // a confirmation on top of it takes Escape first.
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || confirmRequest()) return;
    e.stopImmediatePropagation();
    close();
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));
  const focus = (b: HTMLButtonElement) => queueMicrotask(() => b.focus());

  const canInstall = () => offer && (state() === "ready" || state() === "installing");
  const canDownload = () => offer && state() === "available";

  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class="dialog update-dialog" role="dialog" aria-modal="true" aria-labelledby="update-title">
        <div class="dialog-body update-body">
          <div class="update-head">
            <img class="update-logo" src="/logo.svg" alt="" width="40" height="40" />
            <div class="update-head-text">
              <h2 id="update-title" class="dialog-title">
                {title()}
              </h2>
              <Show when={subtitle()}>
                <div class="update-sub">{subtitle()}</div>
              </Show>
            </div>
          </div>

          <UpdateVersions from={before()} to={target()} installed={!offer} />

          <Show when={offer}>
            <div class="update-facts">
              <Show when={updateStatus()?.total}>
                {(size) => (
                  <span class="update-fact">
                    <UpdateIcon size={14} />
                    {state() === "downloading"
                      ? t("updates.dialog.progress", {
                          received: fmtCache(updateStatus()?.received ?? 0),
                          total: fmtCache(size()),
                        })
                      : fmtCache(size())}
                  </span>
                )}
              </Show>
              <Show when={state() === "ready" || state() === "installing"}>
                <span class="update-fact verified">
                  <ShieldIcon size={14} />
                  {t("updates.dialog.verified")}
                </span>
              </Show>
              <Show when={updateStatus()?.source}>
                {(source) => (
                  <span class="update-fact">
                    <GlobeIcon size={14} />
                    {t("updates.dialog.source", { source: source() })}
                  </span>
                )}
              </Show>
            </div>
            <Show when={state() === "downloading" || state() === "checking"}>
              <ProgressBar value={progress()} class="update-progress" />
            </Show>
          </Show>

          <ReleaseNotes
            releases={releases()}
            from={before()}
            to={target()}
            notes={offer ? updateStatus()?.notes : null}
            loading={!offer && changelog.loading}
          />

          <Show when={canInstall()}>
            <p class="update-hint">{t("updates.dialog.hint")}</p>
          </Show>
          <Show when={canDownload()}>
            <p class="update-hint">{t("updates.dialog.portable")}</p>
          </Show>
          <Show
            when={
              failure() ??
              (state() === "error" && updateStatus()?.error
                ? { cancelled: false, message: updateStatus()?.error ?? "" }
                : null)
            }
          >
            {(f) => (
              <p class="update-failure" classList={{ cancelled: f().cancelled }} role="alert">
                {f().message}
              </p>
            )}
          </Show>
        </div>

        <div class="dialog-footer update-footer">
          <button
            type="button"
            class="btn ghost update-github"
            disabled={!target()}
            onClick={() => void api.openReleasePage(target())}
          >
            <ExternalIcon />
            {t("updates.dialog.github")}
          </button>
          <Show when={canInstall()}>
            <button type="button" class="btn accent" ref={focus} disabled={busy()} onClick={install}>
              <Show when={busy()} fallback={t("updates.dialog.install")}>
                <span class="spinner on-accent" />
                {t("updates.dialog.starting")}
              </Show>
            </button>
          </Show>
          <Show when={canDownload()}>
            <button
              type="button"
              class="btn accent"
              ref={focus}
              onClick={() => {
                void api.openReleasePage(target());
                close();
              }}
            >
              <ExternalIcon />
              {t("updates.dialog.download")}
            </button>
          </Show>
          <Show
            when={canInstall() || canDownload()}
            fallback={
              <button type="button" class="btn accent" ref={focus} onClick={close}>
                {t("updates.dialog.close")}
              </button>
            }
          >
            <button type="button" class="btn" disabled={busy()} onClick={close}>
              {t("updates.dialog.later")}
            </button>
          </Show>
        </div>
      </div>
    </div>
  );
}
