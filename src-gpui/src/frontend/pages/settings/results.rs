//! What the host's answers to settings commands change on the cards, and
//! where a settings command's failure is shown.
use super::*;
use crate::frontend::recorder;
use crate::frontend::widgets::{HotkeyMessage, HotkeyMode, ToastKind, service_connected};

impl Desktop {
    // ---- Command results --------------------------------------------------------

    pub(in crate::frontend) fn settings_completed(
        &mut self,
        command: &str,
        value: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            "manage_privileged_service" => {
                self.ui.busy.remove("privilege");
                self.ui.errors.remove("privilegeStatus");
                self.data.insert("privilege".into(), value.clone());
                let action = self.ui.errors.remove("privilegeAction").unwrap_or_default();
                if action == "uninstall" {
                    // A registration another program still holds is on its
                    // way out: the card says what it waits for.
                    if text(value, "state") == "notInstalled" {
                        self.notify_toast(
                            ToastKind::Success,
                            label("settings.privilege.uninstalled"),
                        );
                    }
                } else if service_connected(value) {
                    self.notify_toast(ToastKind::Success, label("settings.privilege.ready"));
                }
                self.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
            }
            "open_native_task_manager" => {
                self.ui.busy.remove("nativeTaskmgr");
            }
            "manage_task_manager" => {
                self.ui.busy.remove("taskmgr");
                self.data.insert("taskmgr".into(), value.clone());
                let action = self.ui.errors.remove("taskmgrAction").unwrap_or_default();
                self.notify_toast(
                    ToastKind::Success,
                    label(if action == "restore" {
                        "settings.taskManager.restored"
                    } else {
                        "settings.taskManager.enabled"
                    }),
                );
            }
            "set_kernel_sensors" => {
                self.ui.busy.remove("sensors");
            }
            "administrator_status" => {
                self.ui.busy.remove("administratorStatus");
                self.ui.errors.remove("administratorStatus");
            }
            "set_administrator_mode" => {
                self.ui.busy.remove("administrator");
                self.data.insert("administrator".into(), value.clone());
            }
            "clear_history" => {
                self.ui.busy.remove("clearHistory");
                // What the programs seen were goes with the records.
                self.history_forget_kinds();
                let generation = self
                    .data
                    .get("watchGeneration")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    + 1;
                self.data
                    .insert("watchGeneration".into(), json!(generation));
                self.data.insert("processWatch".into(), Value::Null);
                self.data.remove("watchError");
                self.notify_toast_for(
                    ToastKind::Success,
                    label("settings.data.history.cleared"),
                    2500,
                );
                self.data.remove("historyUsage");
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            // What a day takes and what is kept follow the options.
            "set_history_options" => {
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "history_move" | "history_reset_dir" => {
                self.ui.busy.remove("historyMove");
                self.data.remove("historyMoveTo");
                self.notify_toast(
                    ToastKind::Success,
                    label("settings.data.history.place.moved"),
                );
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "set_hotkey" => {
                if self.ui.hotkey.mode == HotkeyMode::Saving {
                    let saved = text(value, "hotkey").to_owned();
                    self.succeed_hotkey(false, saved, window, cx);
                }
            }
            "set_hotkey_enabled" => {
                self.ui.hotkey.switching = false;
                if self.ui.errors.remove("hotkeyOff").is_some() {
                    self.succeed_hotkey(true, String::new(), window, cx);
                }
            }
            "install_update" => {
                self.ui.busy.remove("install");
            }
            "set_update_settings" => {
                let proxy = text(value, "updateProxy");
                if crate::updater::PROXY_PRESETS.contains(&proxy) {
                    self.ui.proxy_draft = None;
                    self.ui.inputs.remove("proxyDraft");
                }
            }
            _ => {}
        }
    }

    /// Shows a settings command's failure on its card; false when it is not one.
    pub(in crate::frontend) fn settings_failed(
        &mut self,
        command: &str,
        _args: &Value,
        error: &Value,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let wrapped = |key: &str| argus_core::i18n::format(key, &[("error", &message)]);
        match command {
            "manage_privileged_service" => {
                self.ui.busy.remove("privilege");
                self.ui.errors.remove("privilegeAction");
                self.ui
                    .errors
                    .insert("privilege".into(), message.to_owned());
                self.data.remove("privilege");
                self.request(
                    "privileged_service_status",
                    json!({}),
                    Some("privilege"),
                    window,
                    cx,
                );
            }
            // Read in the background every half minute, for the badge: a
            // failure belongs on the card, which stops waiting for an answer,
            // not in a notice each time.
            "privileged_service_status" => {
                self.data.insert("privilege".into(), Value::Null);
                self.ui
                    .errors
                    .insert("privilegeStatus".into(), message.to_owned());
            }
            "open_native_task_manager" => {
                self.ui.busy.remove("nativeTaskmgr");
                self.ui.errors.insert(
                    "taskmgr".into(),
                    tr!("settings.taskManager.openNativeFailed", error = message),
                );
            }
            "manage_task_manager" => {
                self.ui.busy.remove("taskmgr");
                self.ui.errors.remove("taskmgrAction");
                self.ui.errors.insert("taskmgr".into(), message.to_owned());
                self.data.remove("taskmgr");
                self.request(
                    "task_manager_status",
                    json!({}),
                    Some("taskmgr"),
                    window,
                    cx,
                );
            }
            "set_kernel_sensors" => {
                self.ui.busy.remove("sensors");
                self.ui.errors.insert("sensors".into(), message.to_owned());
            }
            "administrator_status" => {
                self.ui.busy.remove("administratorStatus");
                self.ui
                    .errors
                    .insert("administratorStatus".into(), message.to_owned());
            }
            "set_administrator_mode" => {
                // The card reads the state again, showing it as busy meanwhile.
                self.ui.busy.remove("administrator");
                self.ui
                    .errors
                    .insert("administrator".into(), message.to_owned());
                self.ui.busy.insert("administratorStatus".into());
                self.request(
                    "administrator_status",
                    json!({}),
                    Some("administrator"),
                    window,
                    cx,
                );
            }
            "complete_onboarding" | "apply_administrator_mode" => {
                self.ui.busy.remove("onboarding");
                self.notify_toast(ToastKind::Error, wrapped("onboarding.saveFailed"));
            }
            "clear_history" => {
                self.ui.busy.remove("clearHistory");
                self.notify_toast(
                    ToastKind::Error,
                    wrapped("settings.data.history.clearError"),
                );
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "set_hotkey" | "set_hotkey_enabled" => {
                self.ui.hotkey.switching = false;
                let off = self.ui.errors.remove("hotkeyOff").is_some();
                let kind = text(error, "kind").to_owned();
                self.ui.hotkey.message = Some(HotkeyMessage::Error(error.clone()));
                if command == "set_hotkey"
                    && self.ui.hotkey.mode == HotkeyMode::Saving
                    && matches!(kind.as_str(), "taken" | "failed" | "invalid")
                {
                    // Try another combination, with the modifiers still down kept.
                    let held = &self.ui.hotkey.recording;
                    let from = recorder::Recording {
                        mods: held.mods,
                        chord: held
                            .chord
                            .iter()
                            .filter(|k| matches!(k, recorder::ChordKey::Modifier(_)))
                            .cloned()
                            .collect(),
                        ..Default::default()
                    };
                    self.listen_hotkey(Some(from), window, cx);
                } else if self.ui.hotkey.mode != HotkeyMode::Idle || off {
                    self.finish_hotkey(true, true, window, cx);
                }
            }
            "install_update" => {
                self.ui.busy.remove("install");
                if boolean(error, "cancelled") {
                    self.notify_toast_for(ToastKind::Info, message.to_owned(), 4000);
                } else {
                    self.ui.errors.insert("install".into(), message.to_owned());
                }
            }
            "set_language" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.language.error"))
            }
            "set_theme" => self.notify_toast(ToastKind::Error, wrapped("settings.theme.error")),
            "set_autostart" => self.notify_toast(
                ToastKind::Error,
                wrapped("settings.startup.autostart.error"),
            ),
            "set_close_to_tray" => self.notify_toast(
                ToastKind::Error,
                wrapped("settings.startup.closeToTray.error"),
            ),
            "set_background_intervals" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.data.background.error"))
            }
            "set_history_enabled" | "set_history_options" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.data.history.error"))
            }
            // The host's message says what to check.
            "history_move" | "history_reset_dir" => {
                self.ui.busy.remove("historyMove");
                self.notify_toast(ToastKind::Error, message.to_owned());
                self.request("history_usage", json!({}), Some("historyUsage"), window, cx);
            }
            "history_open_dir" => self.notify_toast(ToastKind::Error, message.to_owned()),
            "set_rules" => self.notify_toast(ToastKind::Error, wrapped("settings.rules.error")),
            "set_update_settings" | "set_whats_new" => {
                self.notify_toast(ToastKind::Error, wrapped("settings.update.error"))
            }
            "set_hardware_acceleration" => self.notify_toast(
                ToastKind::Error,
                wrapped("settings.motion.acceleration.error"),
            ),
            _ => return false,
        }
        cx.notify();
        true
    }
}
