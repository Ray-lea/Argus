//! The embedded browser renders Argus assets only, never an external page.
//! Keep this boundary while Evergreen is waiting for an upstream security fix.

use tauri::{Manager, Runtime, Url, plugin::TauriPlugin};

fn allows_navigation(url: &Url, use_https_scheme: bool, dev_url: Option<&Url>) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    // Windows maps Tauri's application protocol onto one configured HTTP(S)
    // origin. The other scheme must not fall through to a network request.
    let bundled = url.port().is_none()
        && url.host_str() == Some("tauri.localhost")
        && url.scheme() == if use_https_scheme { "https" } else { "http" };
    bundled
        || dev_url.is_some_and(|dev| {
            matches!(dev.scheme(), "http" | "https") && url.origin() == dev.origin()
        })
}

pub(crate) fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("webview-policy")
        .on_navigation(|webview, url| {
            // This hook covers both config-created windows and the isolated
            // profile used when another account supplies UAC credentials.
            let dev_url = if cfg!(debug_assertions) {
                webview.config().build.dev_url.as_ref()
            } else {
                None
            };
            let allowed = webview
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == webview.label())
                .is_some_and(|window| allows_navigation(url, window.use_https_scheme, dev_url));
            if !allowed {
                // URLs can contain account data or tokens. Do not log them.
                argus_core::diagnostics::warn(
                    "WebView navigation",
                    "blocked navigation outside the application origin",
                );
            }
            allowed
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pages_can_reload_and_navigate_within_the_application() {
        for value in [
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html#settings",
        ] {
            assert!(allows_navigation(&Url::parse(value).unwrap(), false, None));
        }
        let https = Url::parse("https://tauri.localhost/").unwrap();
        assert!(allows_navigation(&https, true, None));
        assert!(!allows_navigation(&https, false, None));
        assert!(!allows_navigation(
            &Url::parse("http://tauri.localhost/").unwrap(),
            true,
            None
        ));
    }

    #[test]
    fn external_documents_and_lookalike_origins_are_rejected() {
        for value in [
            "https://example.com/",
            "http://127.0.0.1:5173/",
            "http://tauri.localhost.example.com/",
            "http://tauri.localhost:5173/",
            "http://tauri.localhost@evil.example/",
            "http://user@tauri.localhost/",
            "tauri://evil.example/",
            "tauri://localhost/",
            "tauri://localhost:5173/",
            "file:///C:/Windows/test.html",
            "data:text/html,<script>alert(1)</script>",
            "javascript:alert(1)",
            "blob:http://tauri.localhost/uuid",
            "about:blank",
        ] {
            assert!(!allows_navigation(&Url::parse(value).unwrap(), false, None));
        }
    }

    #[test]
    fn development_accepts_only_the_configured_origin() {
        let dev = Url::parse("http://localhost:5173/").unwrap();
        assert!(allows_navigation(
            &Url::parse("http://localhost:5173/src/main.tsx").unwrap(),
            false,
            Some(&dev)
        ));
        for value in [
            "http://localhost:5174/",
            "https://localhost:5173/",
            "http://localhost.example.com:5173/",
            "http://127.0.0.1:5173/",
        ] {
            assert!(!allows_navigation(
                &Url::parse(value).unwrap(),
                false,
                Some(&dev)
            ));
        }
    }
}
