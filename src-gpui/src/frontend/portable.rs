//! What a portable copy says about itself: beside the onboarding's first step
//! (`welcome.rs`), one notice in the lower-right stack that tells what the
//! copy lacks beside the installed version and leads to the installer on the
//! website and on GitHub. It comes once per account: after the first start's
//! onboarding, or at the next start of a copy that was onboarded before.
use super::*;
use widgets::{Handler, ToastKind};

/// Set once the notice came in.
const SHOWN: &str = "portable.noticed";
/// How long the window has to be free, the onboarding behind it, before the
/// notice comes in.
const SETTLE: Duration = Duration::from_millis(1500);
/// How long the notice stays when left alone, in ms.
const STAY: u64 = 30_000;

impl Desktop {
    /// This is the standalone exe, not the installation.
    pub(in crate::frontend) fn portable(&self) -> bool {
        boolean(&self.data["info"], "portable")
    }

    /// Each second: brings the notice in on a portable copy that has not
    /// shown it yet, once the onboarding is saved as done and the window is
    /// free. Not while a deferred administrator choice may still restart
    /// Argus, which would take the notice with it; it then comes in after
    /// that restart.
    pub(in crate::frontend) fn announce_portable(&mut self) -> bool {
        if self.options.test || !self.portable() || self.prefs.get(SHOWN, false) {
            return false;
        }
        let visible = self
            .host
            .get_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
        let onboarded = number(&self.data["settings"], "onboarding")
            >= number(welcome::configuration(), "onboardingVersion");
        let switching = self.ui.busy.contains("onboarding")
            || self
                .pending
                .values()
                .any(|pending| pending.command == "apply_administrator_mode");
        let ready = visible
            && self.welcome_checked
            && onboarded
            && !switching
            && self.modal.is_none()
            && !self.snapshot.is_null();
        if !ready {
            self.ui.portable_ready = None;
            return false;
        }
        if self
            .ui
            .portable_ready
            .get_or_insert_with(Instant::now)
            .elapsed()
            < SETTLE
        {
            return false;
        }
        self.ui.portable_ready = None;
        self.prefs.set(SHOWN, true);
        // One request is enough for a run.
        self.skip_star_this_run();
        self.show_portable_notice();
        true
    }

    /// Brings the notice in, whatever was shown before.
    pub(in crate::frontend) fn show_portable_notice(&mut self) {
        let open = |command: &'static str| -> Handler {
            Rc::new(
                move |this: &mut Desktop, window: &mut Window, cx: &mut Context<Desktop>| {
                    this.request(command, json!({}), None, window, cx)
                },
            )
        };
        self.toast_titled(
            ToastKind::Info,
            Some(label("shell.portable.title")),
            label("shell.portable.body"),
            STAY,
            vec![
                (label("shell.portable.website"), open("open_download_page")),
                (label("shell.portable.github"), open("open_latest_release")),
            ],
        );
    }
}
