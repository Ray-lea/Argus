//! Per-instance WebView2 identity. Executable metadata alone is shared by all hosts.

use std::sync::Arc;

use serde::Serialize;
use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows::core::PCWSTR;

use crate::meta::{FileMeta, MetaStore};
use crate::util::{ticks_to_unix_ms, wide};

pub(crate) type Instance = (u32, i64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WebViewRole {
    Browser,
    Renderer,
    Gpu,
    Network,
    Storage,
    Audio,
    Utility,
    Crashpad,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebViewInfo {
    /// A runtime group, not every process with the same executable or host name.
    pub browser_pid: u32,
    pub browser_create_time: i64,
    pub host_name: Option<Arc<str>>,
    pub host_description: Option<Arc<str>>,
    pub host_icon: Option<u32>,
    pub role: WebViewRole,
}

pub(crate) fn is_webview(name: &str) -> bool {
    name.eq_ignore_ascii_case("msedgewebview2.exe")
}

#[derive(Default)]
pub(crate) struct Hint {
    pub host: Option<Arc<str>>,
    role: WebViewRole,
}

fn arguments(line: &str) -> Vec<String> {
    let line = wide(line);
    let mut count = 0;
    unsafe {
        let argv = CommandLineToArgvW(PCWSTR(line.as_ptr()), &mut count);
        if argv.is_null() {
            return Vec::new();
        }
        let args = std::slice::from_raw_parts(argv, count as usize)
            .iter()
            .map(|s| s.to_string().unwrap_or_default())
            .collect();
        let _ = LocalFree(Some(HLOCAL(argv.cast())));
        args
    }
}

impl Hint {
    fn parse(line: &str) -> Self {
        let args = arguments(line);
        // Chromium accepts switches only before the separator. Conflicting
        // duplicates are not useful evidence of the app that started it.
        let value = |key: &str| {
            let mut values = args
                .iter()
                .skip(1)
                .take_while(|v| *v != "--")
                .filter_map(|v| v.strip_prefix(key));
            let first = values.next()?;
            values.all(|v| v == first).then_some(first)
        };
        let host = value("--webview-exe-name=").filter(|name| {
            !name.trim().is_empty()
                && !matches!(*name, "." | "..")
                && name.chars().count() <= 255
                && !name
                    .chars()
                    .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        });
        let has_type = args
            .iter()
            .skip(1)
            .take_while(|v| *v != "--")
            .any(|v| v.starts_with("--type="));
        let role = match value("--type=") {
            None if !has_type && value("--embedded-browser-webview=") == Some("1") => {
                WebViewRole::Browser
            }
            Some("renderer") => WebViewRole::Renderer,
            Some("gpu-process") => WebViewRole::Gpu,
            Some("crashpad-handler") => WebViewRole::Crashpad,
            Some("utility") => match value("--utility-sub-type=") {
                Some("network.mojom.NetworkService") => WebViewRole::Network,
                Some("storage.mojom.StorageService") => WebViewRole::Storage,
                Some("audio.mojom.AudioService") => WebViewRole::Audio,
                _ => WebViewRole::Utility,
            },
            _ => WebViewRole::Unknown,
        };
        Self {
            host: host.map(Arc::from),
            role,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Context {
    pub browser: Instance,
    host_name: Option<Arc<str>>,
    /// Only from a verified live parent; never open a path taken from a switch.
    host_path: Option<Arc<str>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Node<'a> {
    pub instance: Instance,
    pub parent: u32,
    pub session: u32,
    pub name: &'a str,
    pub path: Option<&'a Arc<str>>,
    pub runtime: Option<&'a State>,
}

pub(crate) fn resolve<'a>(start: Node<'a>, get: impl Fn(u32) -> Option<Node<'a>>) -> Context {
    let mut root = start;
    let mut visited = [0; 64];
    let mut host = None;
    for depth in 0..visited.len() {
        visited[depth] = root.instance.0;
        let Some(parent) = get(root.parent)
            .filter(|p| p.instance.1 <= root.instance.1 && p.session == root.session)
        else {
            break;
        };
        if visited[..=depth].contains(&parent.instance.0) || depth == visited.len() - 1 {
            root = start;
            break;
        }
        if parent.runtime.is_none() {
            host = Some(parent);
            break;
        }
        if root
            .runtime
            .is_some_and(|s| s.hint.role == WebViewRole::Browser)
            || root
                .path
                .zip(parent.path)
                .is_some_and(|(a, b)| !a.eq_ignore_ascii_case(b))
        {
            break;
        }
        root = parent;
    }
    let previous = root.runtime.and_then(|s| s.context.as_ref());
    if let Some(previous) = previous.filter(|p| p.browser != root.instance) {
        // The browser exited after it was observed. Keep its original identity
        // for surviving helpers, even if Windows has reused its PID.
        return previous.clone();
    }
    let declared = root
        .runtime
        .and_then(|s| s.hint.host.clone())
        .or_else(|| previous.and_then(|p| p.host_name.clone()))
        .or_else(|| start.runtime.and_then(|s| s.hint.host.clone()));
    let path = host
        .filter(|p| {
            declared
                .as_ref()
                .is_some_and(|name| p.name.eq_ignore_ascii_case(name))
        })
        .and_then(|p| p.path.cloned())
        .or_else(|| {
            previous
                .filter(|p| p.host_name == declared)
                .and_then(|p| p.host_path.clone())
        });
    Context {
        browser: root.instance,
        host_name: declared,
        host_path: path,
    }
}

#[derive(Default)]
pub(crate) struct State {
    hint: Hint,
    attempts: u8,
    context: Option<Context>,
    meta: Option<Arc<FileMeta>>,
    view: Option<Arc<WebViewInfo>>,
}

impl State {
    pub fn learn_context(&mut self, context: &Context) {
        if self.hint.host.is_none()
            && context.host_path.is_some()
            && self
                .context
                .as_ref()
                .is_some_and(|old| old.browser == context.browser && old.host_path.is_none())
        {
            self.context = Some(context.clone());
        }
    }

    pub fn query(&mut self, handle: Option<HANDLE>) {
        if self.attempts >= 3 {
            return;
        }
        let Some(handle) = handle else {
            self.attempts = 3;
            return;
        };
        self.query_with(|| crate::details::command_line(handle));
    }

    fn query_with(&mut self, read: impl FnOnce() -> Option<String>) {
        if self.attempts >= 3 {
            return;
        }
        self.attempts += 1;
        if let Some(line) = read() {
            self.hint = Hint::parse(&line);
            self.attempts = 3;
        }
    }

    /// `app`: the host's packaged app name, which takes the place of its file description.
    pub fn view(
        &mut self,
        context: Context,
        meta: &MetaStore,
        app: Option<Arc<str>>,
    ) -> Arc<WebViewInfo> {
        if self.context.as_ref().and_then(|c| c.host_path.as_ref()) != context.host_path.as_ref() {
            self.meta = None;
        }
        if self.meta.is_none() {
            self.meta = context
                .host_path
                .as_deref()
                .and_then(|path| meta.get_or_request(path));
        }
        let next = WebViewInfo {
            browser_pid: context.browser.0,
            browser_create_time: ticks_to_unix_ms(context.browser.1),
            host_name: context.host_name.clone(),
            host_description: app
                .or_else(|| self.meta.as_ref().and_then(|m| m.description.clone())),
            host_icon: self.meta.as_ref().and_then(|m| m.icon),
            role: self.hint.role,
        };
        self.context = Some(context);
        if self.view.as_deref() != Some(&next) {
            self.view = Some(Arc::new(next));
        }
        self.view.as_ref().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(line: &str) -> State {
        State {
            hint: Hint::parse(line),
            attempts: 3,
            ..State::default()
        }
    }
    struct Entry {
        id: Instance,
        parent: u32,
        session: u32,
        name: &'static str,
        path: Option<Arc<str>>,
        runtime: Option<State>,
    }
    impl Entry {
        fn node(&self) -> Node<'_> {
            Node {
                instance: self.id,
                parent: self.parent,
                session: self.session,
                name: self.name,
                path: self.path.as_ref(),
                runtime: self.runtime.as_ref(),
            }
        }
    }
    fn fixture() -> Vec<Entry> {
        vec![
            Entry {
                id: (10, 100),
                parent: 1,
                session: 1,
                name: "Example Host.exe",
                path: Some(r"C:\Apps\Example Host.exe".into()),
                runtime: None,
            },
            Entry {
                id: (11, 110),
                parent: 10,
                session: 1,
                name: "msedgewebview2.exe",
                path: Some(r"C:\Runtime\msedgewebview2.exe".into()),
                runtime: Some(hint(
                    r#"msedgewebview2.exe --embedded-browser-webview=1 --webview-exe-name="Example Host.exe""#,
                )),
            },
            Entry {
                id: (12, 120),
                parent: 11,
                session: 1,
                name: "msedgewebview2.exe",
                path: Some(r"C:\Runtime\msedgewebview2.exe".into()),
                runtime: Some(hint("msedgewebview2.exe --type=crashpad-handler")),
            },
        ]
    }
    fn context(entries: &[Entry], i: usize) -> Context {
        resolve(entries[i].node(), |pid| {
            entries.iter().find(|e| e.id.0 == pid).map(Entry::node)
        })
    }

    #[test]
    fn parses_quoted_unicode_hosts_and_runtime_roles() {
        let hint = Hint::parse(
            r#""C:\Program Files\Runtime\msedgewebview2.exe" --embedded-browser-webview=1 --webview-exe-name="示例 Host.exe""#, // i18n-ignore
        );
        assert_eq!(hint.host.as_deref(), Some("示例 Host.exe")); // i18n-ignore
        assert_eq!(hint.role, WebViewRole::Browser);
        for (args, role) in [
            ("--type=renderer", WebViewRole::Renderer),
            ("--type=gpu-process", WebViewRole::Gpu),
            ("--type=crashpad-handler", WebViewRole::Crashpad),
            (
                "--type=utility --utility-sub-type=network.mojom.NetworkService",
                WebViewRole::Network,
            ),
            (
                "--type=utility --utility-sub-type=storage.mojom.StorageService",
                WebViewRole::Storage,
            ),
            (
                "--type=utility --utility-sub-type=audio.mojom.AudioService",
                WebViewRole::Audio,
            ),
            (
                "--type=utility --utility-sub-type=future.Service",
                WebViewRole::Utility,
            ),
        ] {
            assert_eq!(
                Hint::parse(&format!("msedgewebview2.exe {args}")).role,
                role
            );
        }
    }

    #[test]
    fn ignores_paths_conflicting_switches_and_arguments_after_separator() {
        for args in [
            r#"--webview-exe-name="C:\Other\app.exe""#,
            "--webview-exe-name=app.exe --webview-exe-name=other.exe",
            "-- --webview-exe-name=app.exe",
            "--webview-exe-name=",
            "--webview-exe-name=..",
            r#"--webview-exe-name="   ""#,
        ] {
            assert!(
                Hint::parse(&format!("msedgewebview2.exe {args}"))
                    .host
                    .is_none()
            );
        }
        assert_eq!(
            Hint::parse(
                "msedgewebview2.exe --embedded-browser-webview=1 --type=renderer --type=gpu-process"
            )
            .role,
            WebViewRole::Unknown
        );
    }

    #[test]
    fn crash_handler_inherits_the_verified_browser_host() {
        let entries = fixture();
        let context = context(&entries, 2);
        assert_eq!(context.browser, (11, 110));
        assert_eq!(context.host_name.as_deref(), Some("Example Host.exe"));
        assert_eq!(context.host_path, entries[0].path);
    }

    #[test]
    fn reused_parent_pid_and_other_sessions_cannot_supply_host_metadata() {
        let mut entries = fixture();
        entries[0].id.1 = 111;
        assert!(context(&entries, 2).host_path.is_none());
        entries[0].id.1 = 100;
        entries[0].session = 2;
        assert!(context(&entries, 2).host_path.is_none());
        entries[0].session = 1;
        entries[0].name = "another.exe";
        assert!(context(&entries, 2).host_path.is_none());
    }

    #[test]
    fn nested_browsers_and_different_runtime_images_remain_separate() {
        let mut entries = fixture();
        entries[2].runtime = Some(hint(
            "msedgewebview2.exe --embedded-browser-webview=1 --webview-exe-name=other.exe",
        ));
        assert_eq!(context(&entries, 2).browser, (12, 120));
        entries[2].runtime = Some(hint("msedgewebview2.exe --type=renderer"));
        entries[2].path = Some(r"D:\OtherRuntime\msedgewebview2.exe".into());
        assert_eq!(context(&entries, 2).browser, (12, 120));
    }

    #[test]
    fn surviving_helpers_keep_the_original_instance_after_browser_exit() {
        let mut entries = fixture();
        let original = context(&entries, 2);
        entries[2].runtime.as_mut().unwrap().context = Some(original.clone());
        entries[1].id.1 = 130;
        assert_eq!(context(&entries, 2).browser, original.browser);
        entries.remove(1);
        assert_eq!(context(&entries, 1).browser, original.browser);
    }

    #[test]
    fn cyclic_parent_links_fall_back_to_the_process_itself() {
        let mut entries = fixture();
        entries[1].runtime = Some(State::default());
        entries[1].parent = 12;
        entries[2].id.1 = 110;
        assert_eq!(context(&entries, 2).browser, entries[2].id);
    }

    #[test]
    fn command_line_is_cached_and_unavailable_data_has_bounded_retries() {
        let mut state = State::default();
        let mut reads = 0;
        for _ in 0..20 {
            state.query_with(|| {
                reads += 1;
                Some("msedgewebview2.exe --type=renderer".into())
            });
        }
        assert_eq!(reads, 1);
        let mut state = State::default();
        reads = 0;
        for _ in 0..20 {
            state.query_with(|| {
                reads += 1;
                None
            });
        }
        assert_eq!(reads, 3);
    }

    #[test]
    fn unchanged_identity_reuses_the_serialized_view() {
        let mut state = hint("msedgewebview2.exe --type=renderer");
        let context = Context {
            browser: (11, 116_444_736_001_100_000),
            host_name: Some("app.exe".into()),
            host_path: None,
        };
        let meta = MetaStore::new();
        let first = state.view(context.clone(), &meta, None);
        let second = state.view(context, &meta, None);
        assert!(Arc::ptr_eq(&first, &second));
        let json = serde_json::to_value(first).unwrap();
        assert_eq!(json["browserPid"], 11);
        assert_eq!(json["browserCreateTime"], 110);
        assert_eq!(json["role"], "renderer");
    }

    #[test]
    fn a_readable_child_can_complete_the_unreadable_browser_host() {
        let mut entries = fixture();
        entries[1].runtime = Some(State::default());
        entries[2].runtime = Some(hint(
            r#"msedgewebview2.exe --type=renderer --webview-exe-name="Example Host.exe""#,
        ));
        let initial = context(&entries, 1);
        entries[1].runtime.as_mut().unwrap().context = Some(initial);
        let learned = context(&entries, 2);
        entries[1].runtime.as_mut().unwrap().learn_context(&learned);
        assert_eq!(
            context(&entries, 1).host_name.as_deref(),
            Some("Example Host.exe")
        );
    }
}
