//! Grouping the process list by user: the preference, its setting, and the
//! groups the rows are built from (`model::UserGroups`).
use super::*;
use model::{UserGrouping, UserGroups};

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
