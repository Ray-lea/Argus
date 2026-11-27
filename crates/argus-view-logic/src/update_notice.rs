//! Where Argus says that an update waits: once per version, in the window's
//! corner while the window is the one in use, otherwise in a Windows
//! notification. With notifications off the notice waits until the window is
//! shown.

/// Where an update is announced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The notice in the window's corner.
    Window,
    /// A Windows notification.
    System,
}

/// The main window as an announcement finds it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    /// Shown and not minimized.
    pub visible: bool,
    /// The foreground window.
    pub active: bool,
}

/// What the announcement is about.
#[derive(Clone, Copy, Debug)]
pub struct Offer<'a> {
    /// The updater's state (`UpdateStatus::state`).
    pub state: &'a str,
    pub version: &'a str,
    /// The version announced last.
    pub announced: Option<&'a str>,
    /// The version the user asked not to be reminded of.
    pub dismissed: &'a str,
}

/// Where to announce `offer` now, or nowhere yet. `notify`: Windows
/// notifications are on.
pub fn notice(offer: Offer, shown: Shown, notify: bool) -> Option<Notice> {
    let Offer {
        state,
        version,
        announced,
        dismissed,
    } = offer;
    if version.is_empty() || !matches!(state, "ready" | "available") {
        return None;
    }
    if announced == Some(version) || dismissed == version {
        return None;
    }
    if shown.visible && shown.active {
        Some(Notice::Window)
    } else if notify {
        Some(Notice::System)
    } else if shown.visible {
        Some(Notice::Window)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Notice, Offer, Shown, notice};

    const READY: Offer = Offer {
        state: "ready",
        version: "1.2.0",
        announced: None,
        dismissed: "",
    };
    const HIDDEN: Shown = Shown {
        visible: false,
        active: false,
    };
    const BEHIND: Shown = Shown {
        visible: true,
        active: false,
    };
    const IN_USE: Shown = Shown {
        visible: true,
        active: true,
    };

    #[test]
    fn the_window_in_use_says_it_and_otherwise_a_notification_does() {
        assert_eq!(notice(READY, IN_USE, true), Some(Notice::Window));
        assert_eq!(notice(READY, BEHIND, true), Some(Notice::System));
        assert_eq!(notice(READY, HIDDEN, true), Some(Notice::System));
        let portable = Offer {
            state: "available",
            ..READY
        };
        assert_eq!(notice(portable, HIDDEN, true), Some(Notice::System));
    }

    #[test]
    fn without_notifications_the_notice_waits_for_the_window() {
        assert_eq!(notice(READY, HIDDEN, false), None);
        assert_eq!(notice(READY, BEHIND, false), Some(Notice::Window));
        assert_eq!(notice(READY, IN_USE, false), Some(Notice::Window));
    }

    #[test]
    fn each_version_is_announced_once_and_never_after_dismissal() {
        let announced = Offer {
            announced: Some("1.2.0"),
            ..READY
        };
        assert_eq!(notice(announced, HIDDEN, true), None);
        assert_eq!(notice(announced, IN_USE, true), None);
        let newer = Offer {
            version: "1.3.0",
            ..announced
        };
        assert_eq!(notice(newer, HIDDEN, true), Some(Notice::System));
        let dismissed = Offer {
            dismissed: "1.2.0",
            ..READY
        };
        assert_eq!(notice(dismissed, HIDDEN, true), None);
    }

    #[test]
    fn only_an_offered_version_is_announced() {
        for state in [
            "idle",
            "checking",
            "upToDate",
            "downloading",
            "installing",
            "error",
        ] {
            let offer = Offer { state, ..READY };
            assert_eq!(notice(offer, HIDDEN, true), None, "{state}");
            assert_eq!(notice(offer, IN_USE, true), None, "{state}");
        }
        let unknown = Offer {
            version: "",
            ..READY
        };
        assert_eq!(notice(unknown, IN_USE, true), None);
    }
}
