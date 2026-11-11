//! Grouping the process list by user: the preference, its setting, and the
//! groups the rows are built from (`model::UserGroups`).
use super::*;
use model::{UserGrouping, UserGroups};
use widgets::ToastKind;

impl Desktop {
    /// The saved `groupByUser` choice; automatic until one is saved.
    pub(in crate::frontend) fn user_grouping(&self) -> UserGrouping {
        UserGrouping::parse(&self.prefs.get(model::USER_GROUPING, Value::Null))
    }

    /// The users the process list is grouped by, if the choice groups it for
    /// the users signed in now.
    pub(in crate::frontend) fn user_groups(&self) -> Option<UserGroups> {
        UserGroups::grouping(values(&self.snapshot["sessions"]), self.user_grouping())
    }

    /// The key of the row that stands for a process's app in the apps view.
    pub(in crate::frontend) fn app_row_key(&self, process: &Value) -> String {
        let key = model::application_key(process);
        match self.user_groups() {
            Some(users) => model::scoped_key(&key, users.id_of(process)),
            None => key,
        }
    }

    /// Folds or unfolds a user's group and keeps the choice.
    pub(in crate::frontend) fn fold_user(&mut self, group: &str, folded: bool) {
        let changed = if folded {
            self.user_folds.insert(group.to_owned())
        } else {
            self.user_folds.remove(group)
        };
        if changed {
            let mut folds = self.user_folds.iter().collect::<Vec<_>>();
            folds.sort();
            self.prefs.set(model::USER_FOLDS, folds);
        }
    }

    /// What a user's heading offers below folding, as the Users page of Task
    /// Manager does: Control Panel's User Accounts, and signing the user out
    /// (asked about first). Nothing for the system's group.
    pub(in crate::frontend) fn user_menu(&self, row: &Row) -> Vec<MenuItem> {
        let Some(heading) = row.user.filter(|heading| !heading.system) else {
            return Vec::new();
        };
        let Some(users) = self.user_groups() else {
            return Vec::new();
        };
        let sessions = users.sessions_of(row.key.strip_prefix("u:").unwrap_or(&row.key));
        // The account behind the group: all of its sessions name it.
        let Some(account) = values(&self.snapshot["sessions"]).iter().find(|session| {
            session["id"]
                .as_u64()
                .is_some_and(|id| sessions.contains(&id))
        }) else {
            return Vec::new();
        };
        // Windows lets a user end their own session; another user's takes
        // administrator rights, which only an elevated Argus has.
        let allowed = heading.current || boolean(&self.data["info"], "elevated");
        let mut sign_out = MenuItem::new(
            label("processes.user.signOut.action"),
            Action::Dialog(
                "signOut".into(),
                json!({
                    "name": row.title,
                    "user": account["user"],
                    "domain": account["domain"],
                    "sessions": sessions,
                    "current": heading.current,
                }),
            ),
        )
        .icon("SignOutIcon")
        .disabled(!allowed);
        // A danger entry keeps its colour when disabled: left plain here, so
        // that it reads as unavailable beside the reason.
        sign_out = if allowed {
            sign_out.danger()
        } else {
            sign_out.detail(label("processes.user.signOut.needsAdmin"))
        };
        vec![
            MenuItem::separator(),
            MenuItem::new(
                label("processes.user.manage"),
                Action::Invoke("open_user_accounts".into(), json!({})),
            )
            .icon("AccountsIcon"),
            sign_out,
        ]
    }

    /// Signs the user of a confirmed `signOut` dialog out. A fixture never
    /// asks for it; a test control (`resolve`) answers in its place.
    pub(in crate::frontend) fn sign_out_user(
        &mut self,
        args: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ui.busy.contains("signOut") {
            return;
        }
        self.ui.busy.insert("signOut".into());
        self.ui
            .errors
            .insert("signOutName".into(), text(&args, "name").to_owned());
        if self.options.fixture.is_some() {
            return;
        }
        self.request(
            "sign_out_user",
            json!({"user": args["user"], "domain": args["domain"], "sessions": args["sessions"]}),
            None,
            window,
            cx,
        );
    }

    /// The answer to signing a user out: a notice either way. Signing the
    /// own user out ends Argus with the session, usually before it shows.
    pub(in crate::frontend) fn signed_out(&mut self, failure: Option<(&Value, &str)>) {
        self.ui.busy.remove("signOut");
        let name = self.ui.errors.remove("signOutName").unwrap_or_default();
        match failure {
            None => self.notify_toast_for(
                ToastKind::Success,
                tr!("processes.user.signOut.done", name = name),
                2500,
            ),
            Some((error, message)) => {
                let kind = error
                    .as_str()
                    .or_else(|| error.get("kind").and_then(Value::as_str))
                    .unwrap_or_default();
                let text = match kind {
                    "accessDenied" => tr!("processes.user.signOut.denied", name = name),
                    "notFound" => tr!("processes.user.signOut.gone", name = name),
                    _ => tr!(
                        "processes.user.signOut.failed",
                        name = name,
                        error = message
                    ),
                };
                self.notify_toast(ToastKind::Error, text);
            }
        }
    }

    /// The setting's choice: automatic, always or never.
    pub(in crate::frontend) fn user_grouping_setting(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.user_grouping();
        self.select(
            "settings-group-by-user",
            label("settings.userGroups.title"),
            UserGrouping::ALL
                .iter()
                .map(|mode| label(&format!("settings.userGroups.{}", mode.key())))
                .collect(),
            UserGrouping::ALL.iter().position(|mode| *mode == current),
            false,
            move |this, index, window, cx| {
                this.preference(
                    model::USER_GROUPING,
                    json!(UserGrouping::ALL[index].key()),
                    window,
                    cx,
                );
            },
            cx,
        )
    }
}
