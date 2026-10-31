//! Rows of the service, startup and connection lists.
//!
//! A list is built from its source (the page's last read) and what the page
//! shows it with (`ListSpec`), all owned: the connection list, thousands of
//! rows that change with every read and name their owners from every sample,
//! is built off the UI thread, and the UI thread only installs the result
//! (`Desktop::refresh_connections`).
use super::collation::{self, SortKey};
use super::model::{Row, boolean, text, values};
use super::theme::label;
use super::*;
use argus_core::i18n::Locale;
use std::cmp::Ordering;

/// What a connection row shows of the process that owns it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Owner {
    pub name: String,
    pub description: String,
    pub icon: Value,
}

/// Connection owners by PID.
pub(super) type Owners = HashMap<u64, Owner>;

/// The owners of `connections` in a snapshot's process list.
pub(super) fn owners(processes: &[Value], connections: &[Value]) -> Owners {
    let owning: HashSet<u64> = connections
        .iter()
        .filter_map(|c| c["pid"].as_u64())
        .collect();
    processes
        .iter()
        .filter_map(|p| {
            let pid = p["pid"].as_u64().filter(|pid| owning.contains(pid))?;
            Some((
                pid,
                Owner {
                    name: text(p, "name").to_owned(),
                    description: text(p, "description").to_owned(),
                    icon: p["icon"].clone(),
                },
            ))
        })
        .collect()
}

/// What a list page shows its source with.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ListSpec {
    pub page: String,
    /// The first column's key: it sorts by title (connections: by owner).
    pub first: &'static str,
    pub query: String,
    pub filter: String,
    pub sort: String,
    pub descending: bool,
    /// The page's row marks (notes and pins), by mark key.
    pub marks: Value,
    /// Titles and the collation order follow the interface language.
    pub locale: Locale,
}

/// How a row compares in the chosen order, computed once per row.
enum Order {
    Key(SortKey),
    Protocol(SortKey, bool),
    Bool(bool),
    Number(f64),
}
impl Order {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Order::Key(a), Order::Key(b)) => a.cmp(b),
            (Order::Protocol(a, a6), Order::Protocol(b, b6)) => a.cmp(b).then_with(|| a6.cmp(b6)),
            (Order::Bool(a), Order::Bool(b)) => a.cmp(b),
            (Order::Number(a), Order::Number(b)) => a.total_cmp(b),
            // One column gives every row the same kind, except a mix of
            // values compared as text.
            _ => Ordering::Equal,
        }
    }
}

/// The rows of a service, startup or connection list, in the page's order with
/// pinned rows first, as the tables show them.
pub(super) fn list_rows(spec: &ListSpec, source: &[Value], owners: &Owners) -> Vec<Row> {
    let page = spec.page.as_str();
    let network = page == "network";
    let query = spec.query.trim().to_lowercase();
    let filter = spec.filter.as_str();
    let marks = &spec.marks;
    // Listening sockets by owner and port: a connection on such a port is local.
    let listening: HashSet<(Option<u64>, Option<u64>)> = if network {
        source
            .iter()
            .filter(|c| text(c, "state") == "listen")
            .map(|c| (c["pid"].as_u64(), c["localPort"].as_u64()))
            .collect()
    } else {
        HashSet::new()
    };
    // A number is almost always a port or PID: match it exactly, so 80 does not find 8080.
    let number_query = query.parse::<u64>().ok();
    let contains = |value: &str| value.to_lowercase().contains(&query);
    let exited = network.then(|| label("connections.exited"));
    let owner = |raw: &Value| raw["pid"].as_u64().and_then(|pid| owners.get(&pid));
    let mut rows: Vec<Row> = source
        .iter()
        .filter(|raw| match page {
            "services" => filter == "all" || text(raw, "state") == filter,
            "network" => {
                filter == "all"
                    || (filter == "listen" && text(raw, "state") == "listen")
                    || (filter == "established" && text(raw, "state") == "established")
                    || (filter == "udp" && text(raw, "protocol") == "udp")
            }
            _ => true,
        })
        .filter_map(|raw| {
            // Keys, titles and marks come from the source entry; only the rows
            // shown are copied.
            let (key, title, mark_key) = match page {
                "services" => {
                    let key = text(raw, "name").to_owned();
                    // Service names are case-insensitive.
                    let mark = key.to_lowercase();
                    (key, text(raw, "displayName").to_owned(), Some(mark))
                }
                "startup" => {
                    let key = text(raw, "id").to_owned();
                    (key.clone(), text(raw, "name").to_owned(), Some(key))
                }
                "network" => {
                    let key = format!(
                        "{}|{}|{}|{}|{}|{}|{}",
                        text(raw, "protocol"),
                        boolean(raw, "ipv6"),
                        raw["pid"],
                        text(raw, "localAddress"),
                        raw["localPort"],
                        text(raw, "remoteAddress"),
                        raw["remotePort"]
                    );
                    let process = owner(raw);
                    let title = match process {
                        Some(p) if p.description.is_empty() => p.name.clone(),
                        Some(p) => p.description.clone(),
                        None if raw["pid"].as_u64() == Some(0) => "System Idle Process".into(),
                        None => exited.clone().unwrap_or_default(),
                    };
                    let mark = process.map(|process| {
                        let name = process.name.to_lowercase();
                        let protocol = text(raw, "protocol");
                        let local = raw["remoteAddress"].is_null()
                            || listening
                                .contains(&(raw["pid"].as_u64(), raw["localPort"].as_u64()));
                        if local {
                            format!("{protocol}|{name}|local|{}", raw["localPort"])
                        } else {
                            format!(
                                "{protocol}|{name}|remote|{}|{}",
                                text(raw, "remoteAddress"),
                                raw["remotePort"]
                                    .as_u64()
                                    .map(|p| p.to_string())
                                    .unwrap_or_default()
                            )
                        }
                    });
                    (key, title, mark)
                }
                _ => return None,
            };
            let mark = mark_key.as_ref().map(|key| &marks[key]);
            let note = mark.and_then(|m| m["note"].as_str()).map(str::to_owned);
            let pinned = mark.is_some_and(|m| boolean(m, "pinned"));
            if !query.is_empty() {
                let found_note = note.as_deref().is_some_and(contains);
                let found = match page {
                    "services" => {
                        number_query.is_some_and(|n| raw["pid"].as_u64() == Some(n))
                            || contains(text(raw, "name"))
                            || contains(text(raw, "displayName"))
                            || contains(text(raw, "description"))
                            || found_note
                    }
                    "startup" => {
                        contains(text(raw, "name"))
                            || contains(text(raw, "entry"))
                            || contains(text(raw, "publisher"))
                            || contains(text(raw, "command"))
                            || found_note
                    }
                    _ => match number_query {
                        Some(n) => {
                            raw["localPort"].as_u64() == Some(n)
                                || raw["remotePort"].as_u64() == Some(n)
                                || raw["pid"].as_u64() == Some(n)
                        }
                        None => {
                            text(raw, "localAddress").contains(&query)
                                || text(raw, "remoteAddress").contains(&query)
                                || owner(raw)
                                    .is_some_and(|p| contains(&p.name) || contains(&p.description))
                                || found_note
                        }
                    },
                };
                if !found {
                    return None;
                }
            }
            let mut raw = raw.clone();
            if network && let Some(process) = owner(&raw) {
                raw["icon"] = process.icon.clone();
            }
            let mut row = Row::item(raw, key, title);
            row.mark_key = mark_key;
            row.note = note;
            row.pinned = pinned;
            Some(row)
        })
        .collect();
    // The interface's table order: text collates as names do, numbers compare
    // by value, and equal rows keep the source order.
    let key = spec.sort.as_str();
    let mut ordered: Vec<(Order, Row)> = rows
        .drain(..)
        .map(|row| {
            let order = if key == spec.first && network {
                // Owners sort by executable name.
                Order::Key(collation::key(
                    owner(&row.raw).map_or("", |p| p.name.as_str()),
                ))
            } else if key == spec.first {
                Order::Key(collation::key(&row.title))
            } else if key == "protocol" {
                Order::Protocol(
                    collation::key(text(&row.raw, key)),
                    boolean(&row.raw, "ipv6"),
                )
            } else {
                match &row.raw[key] {
                    Value::Bool(value) => Order::Bool(*value),
                    // A missing port compares as 0 next to numbers.
                    Value::Number(value) => Order::Number(value.as_f64().unwrap_or(0.)),
                    Value::Null => Order::Number(f64::NAN),
                    value => Order::Key(collation::key(value.as_str().unwrap_or(""))),
                }
            };
            (order, row)
        })
        .collect();
    // A column of numbers and missing values: the missing ones compare as 0;
    // a column without numbers compares them as empty text.
    let numbers = ordered
        .iter()
        .any(|(order, _)| matches!(order, Order::Number(n) if !n.is_nan()));
    for (order, _) in &mut ordered {
        if let Order::Number(n) = order
            && n.is_nan()
        {
            *order = if numbers {
                Order::Number(0.)
            } else {
                Order::Key(collation::key(""))
            };
        }
    }
    ordered.sort_by(|(a, _), (b, _)| {
        let order = a.cmp(b);
        if spec.descending {
            order.reverse()
        } else {
            order
        }
    });
    let mut rows: Vec<Row> = ordered.into_iter().map(|(_, row)| row).collect();
    rows.sort_by_key(|row| !row.pinned);
    rows
}

/// The connection list's inputs, to tell whether rows built from them are current.
#[derive(Clone, PartialEq)]
pub(super) struct ConnectionInputs {
    /// Which read of the list (`Desktop::connections_read`).
    pub read: u64,
    /// Which owners (`Desktop::owners_read`).
    pub owners: u64,
    pub spec: ListSpec,
}

/// Connection rows built for the network page, shown or not.
pub(super) struct ConnectionRows {
    pub inputs: ConnectionInputs,
    pub rows: Arc<Vec<Row>>,
}

impl Desktop {
    /// Moves a connection list given as data (a fixture, a test control) to
    /// where the network page reads it.
    pub(in crate::frontend) fn take_connections(&mut self) {
        if let Some(list) = self.data.remove("connections") {
            self.set_connections(list);
        }
    }
    /// Keeps a new read of the connection list; returns whether it changed.
    pub(in crate::frontend) fn set_connections(&mut self, list: Value) -> bool {
        if self.connections.as_deref() == Some(&list) {
            return false;
        }
        self.connections = Some(Arc::new(list));
        self.connections_read = self.connections_read.wrapping_add(1);
        true
    }
    /// What a list page's rows are shown with.
    pub(in crate::frontend) fn list_spec(&self, page: &str, first: &'static str) -> ListSpec {
        ListSpec {
            page: page.to_owned(),
            first,
            query: if page == self.page {
                self.query.clone()
            } else {
                self.queries.get(page).cloned().unwrap_or_default()
            },
            filter: self
                .filters
                .get(page)
                .cloned()
                .unwrap_or_else(|| "all".into()),
            sort: if page == self.page {
                self.sort.clone()
            } else {
                self.sorts
                    .get(page)
                    .map(|sort| sort.0.clone())
                    .unwrap_or_else(|| super::tables::saved_sort(page, &self.prefs).0)
            },
            descending: if page == self.page {
                self.descending
            } else {
                self.sorts
                    .get(page)
                    .map(|sort| sort.1)
                    .unwrap_or_else(|| super::tables::saved_sort(page, &self.prefs).1)
            },
            marks: self.marks_of(page),
            locale: argus_core::i18n::locale(),
        }
    }
    /// The connections' owners in the current snapshot, read again only for
    /// a new sample or list, and replaced only when a connection's title,
    /// icon, mark or order could change: other processes come and go all the time.
    fn sync_owners(&mut self) {
        let at = (self.sequence, self.connections_read);
        if self.owners_sequence == Some(at) {
            return;
        }
        self.owners_sequence = Some(at);
        let owners = owners(
            values(&self.snapshot["processes"]),
            self.connections.as_deref().map(values).unwrap_or_default(),
        );
        if *self.owners != owners {
            self.owners = Arc::new(owners);
            self.owners_read = self.owners_read.wrapping_add(1);
        }
    }
    fn connection_inputs(&mut self) -> ConnectionInputs {
        self.sync_owners();
        ConnectionInputs {
            read: self.connections_read,
            owners: self.owners_read,
            spec: self.list_spec("network", super::tables::first_column("network")),
        }
    }
    /// The connections and owners for a build on the UI thread.
    pub(in crate::frontend) fn connection_source(&mut self) -> (Option<Arc<Value>>, Arc<Owners>) {
        self.sync_owners();
        (self.connections.clone(), self.owners.clone())
    }
    /// Records rows the UI thread built for the network page, so that the
    /// page can show them again without building them.
    pub(in crate::frontend) fn keep_connection_rows(&mut self, rows: Arc<Vec<Row>>) {
        let inputs = self.connection_inputs();
        // A build still running started earlier: these rows are newer.
        self.connection_build = self.connection_build.wrapping_add(1);
        self.connection_building = None;
        self.connection_rows = Some(ConnectionRows { inputs, rows });
    }
    /// Shows the network page's last rows if they were built for what the page
    /// shows now (perhaps from an earlier read or sample, which a build off the
    /// UI thread then replaces). Returns whether it did.
    pub(in crate::frontend) fn show_kept_connections(&mut self, cx: &mut Context<Self>) -> bool {
        let spec = self.list_spec("network", super::tables::first_column("network"));
        let Some(rows) = self
            .connection_rows
            .as_ref()
            .filter(|kept| kept.inputs.spec == spec)
            .map(|kept| kept.rows.clone())
        else {
            return false;
        };
        self.install_rows(rows, cx);
        self.refresh_connections(cx);
        true
    }
    /// Builds the connection rows off the UI thread when the list, its owners
    /// or how the page shows it changed since the rows last built, and installs
    /// them if the network page is shown; after its entrance if it is entering.
    pub(in crate::frontend) fn refresh_connections(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.connections.clone() else {
            return;
        };
        let inputs = self.connection_inputs();
        let current = self
            .connection_rows
            .as_ref()
            .is_some_and(|kept| kept.inputs == inputs);
        if current || self.connection_building.as_ref() == Some(&inputs) {
            if current && self.page == "network" {
                self.install_kept_connections(cx);
            }
            return;
        }
        self.connection_build = self.connection_build.wrapping_add(1);
        let build = self.connection_build;
        self.connection_building = Some(inputs.clone());
        let owners = self.owners.clone();
        let spec = inputs.spec.clone();
        let task = cx.background_executor().spawn(async move {
            let started = Instant::now();
            let rows = list_rows(&spec, values(&source), &owners);
            (rows, started.elapsed().as_micros() as u64)
        });
        cx.spawn(async move |this, cx| {
            let (rows, us) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.connection_build != build {
                    return;
                }
                this.connection_building = None;
                this.work.background_list_us += us;
                this.connection_rows = Some(ConnectionRows {
                    inputs,
                    rows: Arc::new(rows),
                });
                if this.page == "network" {
                    this.install_kept_connections(cx);
                }
            });
        })
        .detach();
    }
    /// Installs the kept connection rows on the shown network page, unless
    /// they are already shown. While the page enters it replays its first
    /// frame moved and faded: new rows would draw the whole table again in the
    /// middle of it, so they wait for its end.
    fn install_kept_connections(&mut self, cx: &mut Context<Self>) {
        let Some(rows) = self.connection_rows.as_ref().map(|kept| kept.rows.clone()) else {
            return;
        };
        if Arc::ptr_eq(&rows, &self.rows) {
            return;
        }
        if self.list_page_entering(cx) {
            self.after_entrance(cx);
            return;
        }
        let started = Instant::now();
        self.install_rows(rows, cx);
        super::trace_work(
            "rows",
            Some(&format!("network {} installed", self.rows.len())),
            started,
        );
        // The page's summary counts the list the rows came from.
        cx.notify();
    }
    /// Whether a service, startup or connection list is entering. It replays
    /// its first frame at the animated offset; drawing it again (new rows, or a sample,
    /// which changes nothing there but the status bar) would stall the entrance
    /// for as long as the whole table takes to draw.
    pub(in crate::frontend) fn list_page_entering(&self, cx: &App) -> bool {
        matches!(self.page.as_str(), "services" | "startup" | "network")
            && (self.page_enter_pending || self.page_entering(cx))
    }
    /// Draws the desktop again (and installs the connection rows built
    /// meanwhile) once the page's entrance is over.
    pub(in crate::frontend) fn after_entrance(&mut self, cx: &mut Context<Self>) {
        if self.entrance_queued {
            return;
        }
        self.entrance_queued = true;
        // Before its first frame the entrance has not started.
        let remaining = if self.page_enter_pending {
            widgets::PAGE_ENTER
        } else {
            widgets::PAGE_ENTER.saturating_sub(self.page_switched.elapsed())
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(remaining).await;
            let _ = this.update(cx, |this, cx| {
                this.entrance_queued = false;
                if this.list_page_entering(cx) {
                    // Another page change: wait for its entrance instead.
                    this.after_entrance(cx);
                    return;
                }
                if this.page == "network" {
                    this.install_kept_connections(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // Not the glob: the frontend's imports bring GPUI's own `test` attribute.
    use super::{ListSpec, Row, list_rows, owners};
    use serde_json::{Value, json};

    fn spec(sort: &str, descending: bool, query: &str, marks: Value) -> ListSpec {
        ListSpec {
            page: "network".into(),
            first: "name",
            query: query.into(),
            filter: "all".into(),
            sort: sort.into(),
            descending,
            marks,
            locale: argus_core::i18n::locale(),
        }
    }
    fn connections() -> Vec<Value> {
        vec![
            json!({"protocol":"tcp","ipv6":false,"localAddress":"0.0.0.0","localPort":8080,"remoteAddress":null,"remotePort":null,"state":"listen","pid":10}),
            json!({"protocol":"tcp","ipv6":false,"localAddress":"10.0.0.2","localPort":50000,"remoteAddress":"1.1.1.1","remotePort":443,"state":"established","pid":20}),
            json!({"protocol":"udp","ipv6":false,"localAddress":"0.0.0.0","localPort":80,"remoteAddress":null,"remotePort":null,"state":null,"pid":10}),
            json!({"protocol":"tcp","ipv6":false,"localAddress":"10.0.0.2","localPort":8080,"remoteAddress":"10.0.0.3","remotePort":51000,"state":"established","pid":10}),
        ]
    }
    fn processes() -> Vec<Value> {
        vec![
            json!({"pid":10,"name":"server.exe","description":"","icon":1}),
            json!({"pid":20,"name":"browser.exe","description":"Browser","icon":2}),
            json!({"pid":30,"name":"idle.exe","description":"","icon":3}),
        ]
    }
    fn ports(rows: &[Row]) -> Vec<(u64, Option<u64>)> {
        rows.iter()
            .map(|row| {
                (
                    row.raw["localPort"].as_u64().unwrap_or(0),
                    row.raw["remotePort"].as_u64(),
                )
            })
            .collect()
    }

    #[test]
    fn owners_cover_only_processes_with_connections() {
        let owners = owners(&processes(), &connections());
        assert_eq!(owners.len(), 2);
        assert!(!owners.contains_key(&30));
    }

    #[test]
    fn connections_take_titles_icons_and_marks_from_their_owners() {
        let source = connections();
        let owners = owners(&processes(), &source);
        let rows = list_rows(&spec("localPort", false, "", json!({})), &source, &owners);
        let browser = rows.iter().find(|row| row.raw["pid"] == 20).unwrap();
        assert_eq!(browser.title, "Browser");
        assert_eq!(browser.raw["icon"], 2);
        assert_eq!(
            browser.mark_key.as_deref(),
            Some("tcp|browser.exe|remote|1.1.1.1|443")
        );
        // A connection to a port its owner listens on is local.
        let served = rows
            .iter()
            .find(|row| row.raw["remotePort"] == 51000)
            .unwrap();
        assert_eq!(served.title, "server.exe");
        assert_eq!(
            served.mark_key.as_deref(),
            Some("tcp|server.exe|local|8080")
        );
    }

    #[test]
    fn missing_ports_sort_as_zero_and_equal_rows_keep_their_order() {
        let source = connections();
        let owners = owners(&processes(), &source);
        let rows = list_rows(&spec("remotePort", false, "", json!({})), &source, &owners);
        assert_eq!(
            ports(&rows),
            vec![
                (8080, None),
                (80, None),
                (50000, Some(443)),
                (8080, Some(51000))
            ]
        );
        let rows = list_rows(&spec("localPort", true, "", json!({})), &source, &owners);
        assert_eq!(
            ports(&rows),
            vec![
                (50000, Some(443)),
                (8080, None),
                (8080, Some(51000)),
                (80, None)
            ]
        );
    }

    #[test]
    fn a_number_finds_ports_and_pids_exactly_and_pinned_rows_lead() {
        let source = connections();
        let owners = owners(&processes(), &source);
        let rows = list_rows(&spec("localPort", false, "80", json!({})), &source, &owners);
        assert_eq!(ports(&rows), vec![(80, None)]);
        let rows = list_rows(&spec("localPort", false, "20", json!({})), &source, &owners);
        assert_eq!(ports(&rows), vec![(50000, Some(443))]);
        let marks = json!({"tcp|browser.exe|remote|1.1.1.1|443":{"pinned":true,"note":"vpn"}});
        let rows = list_rows(
            &spec("localPort", false, "", marks.clone()),
            &source,
            &owners,
        );
        assert_eq!(rows[0].raw["pid"], 20);
        assert!(rows[0].pinned);
        // Notes are searched too.
        let rows = list_rows(&spec("localPort", false, "vpn", marks), &source, &owners);
        assert_eq!(ports(&rows), vec![(50000, Some(443))]);
    }
}
