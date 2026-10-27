//! Row context menus, in the original order, with icons, separators and submenus.
use super::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

/// Task Manager's order, highest first.
const PRIORITIES: [&str; 6] = [
    "realtime",
    "high",
    "aboveNormal",
    "normal",
    "belowNormal",
    "idle",
];
impl MenuItem {
    pub(super) fn separator() -> Self {
        let mut item = Self::new(String::new(), Action::CloseOverlay);
        item.separator = true;
        item
    }
    pub(super) fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub(super) fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub(super) fn hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint);
        self
    }
    pub(super) fn submenu(mut self, children: Vec<MenuItem>) -> Self {
        self.children = children;
        self
    }
}
/// Adds `items` to a popup menu; actions run on the desktop after the menu closes.
pub(super) fn fill_menu(
    mut menu: PopupMenu,
    items: Vec<MenuItem>,
    owner: WeakEntity<Desktop>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    menu = menu.min_w(px(220.));
    let p = Palette::current(cx);
    for item in items {
        if item.separator {
            menu = menu.separator();
            continue;
        }
        let glyph = item.icon.map(|name| {
            icon(
                name,
                if item.danger && !item.disabled {
                    p.danger
                } else {
                    p.ink
                },
            )
        });
        if !item.children.is_empty() {
            let children = item.children.clone();
            let owner = owner.clone();
            menu = menu.submenu_with_icon(glyph, item.label, window, cx, move |sub, window, cx| {
                fill_menu(sub, children.clone(), owner.clone(), window, cx)
            });
            continue;
        }
        let entry = if item.danger || item.hint.is_some() {
            let text = item.label.clone();
            let danger = item.danger && !item.disabled;
            let hint = item.hint;
            PopupMenuItem::element(move |_, cx| {
                let p = Palette::current(cx);
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_color(if danger { p.danger } else { p.ink })
                            .child(text.clone()),
                    )
                    .when_some(hint, |d, hint| {
                        d.child(div().text_size(px(12.)).text_color(p.faint).child(hint))
                    })
            })
        } else {
            PopupMenuItem::new(item.label)
        };
        let action = item.action;
        let owner = owner.clone();
        menu = menu.item(
            entry
                .when_some(glyph, |entry, glyph| entry.icon(glyph))
                .disabled(item.disabled)
                .checked(item.checked)
                .on_click(move |_, window, cx| {
                    let owner = owner.clone();
                    let action = action.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |this, cx| this.act(action, window, cx));
                    });
                }),
        );
    }
    menu
}
impl Desktop {
    pub(in crate::frontend) fn row_menu(&self, row: &Row) -> Vec<MenuItem> {
        if row.section.is_some() {
            return vec![MenuItem::new(
                label(if row.expanded {
                    "common.collapse"
                } else {
                    "common.expand"
                }),
                Action::Fold(row.key.clone()),
            )];
        }
        let mut items = match self.page.as_str() {
            "processes" => self.process_menu(row),
            "services" => self.service_menu(row),
            "startup" => {
                let raw = &row.raw;
                let path = raw["path"].as_str().filter(|p| !p.is_empty());
                vec![
                    MenuItem::new(
                        label(if boolean(raw, "enabled") {
                            "common.disable"
                        } else {
                            "common.enable"
                        }),
                        Action::Invoke(
                            "set_startup_enabled".into(),
                            json!({"id":raw["id"],"enabled":!boolean(raw,"enabled")}),
                        ),
                    )
                    .disabled(!raw["blocked"].is_null()),
                    MenuItem::separator(),
                    MenuItem::new(
                        label("common.openFileLocation"),
                        Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
                    )
                    .icon("FolderIcon")
                    .disabled(!boolean(raw, "exists") || path.is_none()),
                    MenuItem::new(
                        label("startup.menu.copyCommand"),
                        Action::Copy(text(raw, "command").into(), "startup.copied.command"),
                    )
                    .icon("CopyIcon")
                    .disabled(text(raw, "command").is_empty()),
                    MenuItem::separator(),
                ]
            }
            "network" => self.connection_menu(row),
            _ => vec![],
        };
        if self.page != "processes" {
            items.extend(self.mark_menu(row));
        }
        items
    }
    fn process_menu(&self, row: &Row) -> Vec<MenuItem> {
        let group = row.members.len() > 1;
        let args = json!({"members":row.members,"name":row.title});
        let mut items = vec![
            MenuItem::new(
                if group {
                    tr!("processes.menu.endAll", count = row.members.len())
                } else {
                    label("common.endProcess")
                },
                Action::Dialog("end".into(), args.clone()),
            )
            .icon("EndIcon")
            .hint("Del")
            .danger(),
        ];
        if !group {
            items.push(
                MenuItem::new(
                    label("common.endProcessTree"),
                    Action::Dialog("endTree".into(), args.clone()),
                )
                .icon("BranchEndIcon"),
            );
        }
        if self.data.get("desktopShell").is_some_and(|shell| {
            row.members.iter().any(|member| {
                shell["pid"] == member["pid"] && shell["createTime"] == member["createTime"]
            })
        }) {
            items.push(
                MenuItem::new(
                    label("tasks.explorer.restart"),
                    Action::Dialog("restartExplorer".into(), row.raw.clone()),
                )
                .icon("RestartIcon"),
            );
        }
        // The priority every process of the row shares, if they agree.
        let priorities = row
            .members
            .iter()
            .map(|member| priority_class(member))
            .collect::<HashSet<_>>();
        let current = (priorities.len() == 1)
            .then(|| priorities.into_iter().next().flatten())
            .flatten();
        let efficient = row
            .members
            .iter()
            .all(|member| boolean(member, "efficiency"));
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(label("processes.menu.setPriority"), Action::CloseOverlay)
                .icon("GaugeIcon")
                .submenu(
                    PRIORITIES
                        .into_iter()
                        .map(|priority| {
                            MenuItem::new(
                                label(&format!("processes.priority.{priority}")),
                                Action::Dialog(
                                    "priority".into(),
                                    json!({"members":row.members,"name":row.title,"priority":priority}),
                                ),
                            )
                            .checked(current == Some(priority))
                        })
                        .collect(),
                ),
        );
        items.push(
            MenuItem::new(
                label("common.efficiencyMode"),
                Action::Dialog(
                    "efficiency".into(),
                    json!({"members":row.members,"name":row.title,"on":!efficient}),
                ),
            )
            .icon("LeafIcon")
            .checked(efficient),
        );
        items.push(MenuItem::new(
            label("processes.menu.addRule"),
            Action::Dialog("ruleScope".into(), json!({"name":text(&row.raw,"name")})),
        ));
        items.push(MenuItem::separator());
        if row.expandable {
            items.push(MenuItem::new(
                label(if row.expanded {
                    "common.collapse"
                } else {
                    "common.expand"
                }),
                Action::Fold(row.key.clone()),
            ));
        }
        let path = row.raw["path"].as_str().filter(|p| !p.is_empty());
        items.push(
            MenuItem::new(
                label("common.openFileLocation"),
                Action::Invoke("reveal_in_explorer".into(), json!({"path":path})),
            )
            .icon("FolderIcon")
            .disabled(path.is_none()),
        );
        items.push(
            MenuItem::new(
                label("common.copyPath"),
                Action::Copy(path.unwrap_or_default().into(), "actions.copyWhat.path"),
            )
            .icon("CopyIcon")
            .disabled(path.is_none()),
        );
        items.push(if group {
            MenuItem::new(
                label("common.copyName"),
                Action::Copy(text(&row.raw, "name").into(), "actions.copyWhat.name"),
            )
        } else {
            MenuItem::new(
                label("processes.menu.copyPid"),
                Action::Copy(row.raw["pid"].to_string(), "processes.menu.copyPidWhat"),
            )
        });
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new(
                label("processes.menu.details"),
                Action::Preference("openDetails".into(), json!(row.key)),
            )
            .icon("InfoIcon")
            .hint("Enter"),
        );
        items
    }
    fn service_menu(&self, row: &Row) -> Vec<MenuItem> {
        let raw = &row.raw;
        let name = text(raw, "name");
        let state = text(raw, "state");
        let pending = state.ends_with("Pending");
        let control = |action: &str| {
            Action::Invoke(
                "control_service".into(),
                json!({"name":name,"action":action}),
            )
        };
        // The binary is the first token of the image path, quoted or not.
        let command = text(raw, "path").trim();
        let binary = command
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .or_else(|| command.split_whitespace().next())
            .filter(|binary| !binary.is_empty());
        let current = text(raw, "startType");
        let mut items = vec![
            MenuItem::new(label("common.start"), control("start"))
                .icon("PlayIcon")
                .disabled(state == "running" || pending),
            MenuItem::new(label("common.stop"), control("stop"))
                .icon("StopIcon")
                .disabled(!boolean(raw, "canStop") || pending),
            MenuItem::new(label("common.restart"), control("restart"))
                .icon("RestartIcon")
                .disabled(!boolean(raw, "canStop") || pending),
            MenuItem::separator(),
        ];
        for start_type in ["auto", "autoDelayed", "manual", "disabled"] {
            items.push(
                MenuItem::new(
                    // `type` is a keyword, so the placeholder is passed by name.
                    argus_core::i18n::format(
                        "services.menu.startType",
                        &[(
                            "type",
                            &label(&format!("services.startType.{start_type}"))
                                as &dyn std::fmt::Display,
                        )],
                    ),
                    Action::Invoke(
                        "set_service_start_type".into(),
                        json!({"name":name,"startType":start_type}),
                    ),
                )
                .checked(current == start_type),
            );
        }
        let pid = raw["pid"].as_u64().unwrap_or(0);
        items.extend([
            MenuItem::separator(),
            MenuItem::new(
                label("common.goToProcess"),
                Action::Preference("revealProcess".into(), json!({"pid":pid})),
            )
            .icon("GoToIcon")
            .disabled(pid == 0),
            MenuItem::new(
                label("common.openFileLocation"),
                Action::Invoke("reveal_in_explorer".into(), json!({"path":binary})),
            )
            .icon("FolderIcon")
            .disabled(binary.is_none()),
            MenuItem::new(
                label("services.menu.copyName"),
                Action::Copy(name.into(), "services.copied.name"),
            )
            .icon("CopyIcon"),
            MenuItem::separator(),
        ]);
        items
    }
    fn connection_menu(&self, row: &Row) -> Vec<MenuItem> {
        let raw = &row.raw;
        let process = values(&self.snapshot["processes"])
            .iter()
            .find(|p| p["pid"] == raw["pid"]);
        let pid = raw["pid"].as_u64().unwrap_or(0);
        let name = process
            .map(|p| text(p, "name").to_owned())
            .unwrap_or_else(|| format!("PID {pid}"));
        let v6 = boolean(raw, "ipv6");
        let endpoint = |address: &str, port: &Value| match port.as_u64() {
            Some(port) if v6 => format!("[{address}]:{port}"),
            Some(port) => format!("{address}:{port}"),
            None => address.to_owned(),
        };
        let closable = text(raw, "protocol") == "tcp" && !v6 && text(raw, "state") == "established";
        let remote = raw["remoteAddress"].as_str();
        vec![
            MenuItem::new(
                label("common.goToProcess"),
                Action::Preference("revealProcess".into(), raw.clone()),
            )
            .icon("GoToIcon")
            .disabled(process.is_none()),
            MenuItem::new(
                tr!("connections.menu.end", name = name),
                Action::Dialog(
                    "end".into(),
                    json!({"members":process.into_iter().collect::<Vec<_>>(),"name":name}),
                ),
            )
            .icon("EndIcon")
            .danger()
            .disabled(pid <= 4 || process.is_none()),
            MenuItem::new(
                label("connections.menu.close"),
                Action::Invoke("close_connection".into(), raw.clone()),
            )
            .icon("DismissIcon")
            .disabled(!closable),
            MenuItem::separator(),
            MenuItem::new(
                label("connections.menu.copyLocal"),
                Action::Copy(
                    endpoint(text(raw, "localAddress"), &raw["localPort"]),
                    "connections.copied.local",
                ),
            )
            .icon("CopyIcon"),
            MenuItem::new(
                label("connections.menu.copyRemote"),
                Action::Copy(
                    remote
                        .map(|r| endpoint(r, &raw["remotePort"]))
                        .unwrap_or_default(),
                    "connections.copied.remote",
                ),
            )
            .disabled(remote.is_none()),
            MenuItem::separator(),
        ]
    }
    /// Pin and note items, kept under the row's mark key.
    fn mark_menu(&self, row: &Row) -> Vec<MenuItem> {
        let key = row.mark_key.clone();
        let pinned = row.pinned;
        let mut items = vec![
            MenuItem::new(
                label(if pinned {
                    "common.marks.unpin"
                } else {
                    "common.marks.pin"
                }),
                Action::Preference("pin".into(), json!(key)),
            )
            .icon(if pinned { "UnpinIcon" } else { "PinIcon" })
            .disabled(key.is_none()),
            MenuItem::new(
                label(if row.note.is_some() {
                    "common.marks.editNote"
                } else {
                    "common.marks.addNote"
                }),
                Action::Dialog(
                    "note".into(),
                    json!({"key":key,"name":row.title,"note":row.note}),
                ),
            )
            .icon("NoteIcon")
            .disabled(key.is_none()),
        ];
        if let (Some(key), Some(_)) = (&row.mark_key, &row.note) {
            items.push(
                MenuItem::new(
                    label("common.marks.deleteNote"),
                    Action::Preference("deleteNote".into(), json!(key)),
                )
                .icon("TrashIcon"),
            );
        }
        items
    }
}
/// The priority class of a base priority; other values match no class.
fn priority_class(process: &Value) -> Option<&'static str> {
    Some(match process["basePriority"].as_i64()? {
        4 => "idle",
        6 => "belowNormal",
        8 => "normal",
        10 => "aboveNormal",
        13 => "high",
        24 => "realtime",
        _ => return None,
    })
}
