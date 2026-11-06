//! Per-executable metadata (version strings, icon) and packaged app names,
//! resolved off the sampling thread and cached by path or app.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::UI::Shell::{
    SHDefExtractIconW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
use windows::core::PCWSTR;

use crate::package::{self, AppMeta, Identity};
use crate::util::wide;

/// Metadata shared by every process running the same executable.
#[derive(Debug, Default)]
pub struct FileMeta {
    pub description: Option<Arc<str>>,
    pub company: Option<Arc<str>>,
    pub windows_component: bool,
    /// Stable icon ID, served to the UI as a PNG.
    pub icon: Option<u32>,
}

/// How often paths no process runs any more are looked for, and how long one
/// stays cached after that: a program that starts again soon (a shell, a
/// compiler) keeps its entry, while the executables a build or an installer
/// runs once do not pile up for as long as Argus runs.
const SWEEP_EVERY: Duration = Duration::from_secs(60);
const KEEP_IDLE: Duration = Duration::from_secs(10 * 60);
const MAX_PENDING_PATHS: usize = 512;
const MAX_PENDING_APPS: usize = 64;
const MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;

struct Cached<T> {
    meta: Arc<T>,
    /// Since when nothing but the store has held `meta`.
    idle_since: Option<Instant>,
}

impl<T> Cached<T> {
    fn keep(&mut self, now: Instant) -> bool {
        if Arc::strong_count(&self.meta) > 1 {
            self.idle_since = None;
            return true;
        }
        let since = *self.idle_since.get_or_insert(now);
        now.saturating_duration_since(since) < KEEP_IDLE
    }
}

#[derive(Default)]
struct Inner {
    by_path: HashMap<String, Cached<FileMeta>>,
    pending: HashMap<String, usize>,
    pending_bytes: usize,
    /// IDs are never reused. A displayed snapshot pins its PNGs until replaced.
    icons: HashMap<u32, Arc<[u8]>>,
    next_icon: u32,
    /// Icon ids by PNG content. Most executables show one of a few icons
    /// (the generic program icon above all), so each image is kept once.
    icon_ids: HashMap<Arc<[u8]>, u32>,
    /// By package full name and app ID: one package version's app.
    apps: HashMap<(Box<str>, Box<str>), Cached<AppMeta>>,
    apps_pending: HashSet<(Box<str>, Box<str>)>,
}

pub struct MetaStore {
    inner: RwLock<Inner>,
    queue: SyncSender<String>,
    /// Apps resolve on their own thread: the shell takes 10–50 ms per app,
    /// which would hold up every executable's icon queued behind it.
    apps: SyncSender<Identity>,
    last_sweep: Mutex<Option<Instant>>,
}

impl MetaStore {
    /// Creates the store and its background resolver threads.
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::sync_channel(MAX_PENDING_PATHS);
        let (apps_tx, apps_rx) = mpsc::sync_channel(MAX_PENDING_APPS);
        let store = Arc::new(Self {
            inner: RwLock::default(),
            queue: tx,
            apps: apps_tx,
            last_sweep: Mutex::new(None),
        });
        let weak = Arc::downgrade(&store);
        let _ = std::thread::Builder::new()
            .name("argus-meta".into())
            .spawn(move || worker(weak, rx))
            .inspect_err(|error| {
                crate::diagnostics::warn("metadata worker creation", crate::diagnostics::io(error))
            });
        let weak = Arc::downgrade(&store);
        let _ = std::thread::Builder::new()
            .name("argus-apps".into())
            .spawn(move || app_worker(weak, apps_rx))
            .inspect_err(|error| {
                crate::diagnostics::warn("app name worker creation", crate::diagnostics::io(error))
            });
        store
    }

    /// Cached metadata for `path`; schedules resolution on a miss.
    pub fn get_or_request(&self, path: &str) -> Option<Arc<FileMeta>> {
        let key = path.to_lowercase();
        {
            let inner = self.inner.read().unwrap();
            if let Some(cached) = inner.by_path.get(&key) {
                return Some(cached.meta.clone());
            }
            if inner.pending.contains_key(&key) {
                return None;
            }
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(cached) = inner.by_path.get(&key) {
            return Some(cached.meta.clone());
        }
        let bytes = key.capacity().saturating_add(path.len());
        if !inner.pending.contains_key(&key)
            && inner.pending.len() < MAX_PENDING_PATHS
            && bytes <= MAX_PENDING_BYTES.saturating_sub(inner.pending_bytes)
            && self.queue.try_send(path.to_owned()).is_ok()
        {
            inner.pending_bytes += bytes;
            inner.pending.insert(key, bytes);
        }
        None
    }

    /// The packaged app a process runs as; schedules resolution on a miss.
    pub(crate) fn app_or_request(&self, identity: &Identity) -> Option<Arc<AppMeta>> {
        let key = (identity.full_name.clone(), identity.aumid.clone());
        {
            let inner = self.inner.read().unwrap();
            if let Some(app) = inner.apps.get(&key) {
                return Some(app.meta.clone());
            }
            if inner.apps_pending.contains(&key) {
                return None;
            }
        }
        let mut inner = self.inner.write().unwrap();
        if let Some(cached) = inner.apps.get(&key) {
            return Some(cached.meta.clone());
        }
        if !inner.apps_pending.contains(&key)
            && inner.apps_pending.len() < MAX_PENDING_APPS
            && self.apps.try_send(identity.clone()).is_ok()
        {
            inner.apps_pending.insert(key);
        }
        None
    }

    /// PNG bytes of a previously resolved icon.
    pub fn icon_png(&self, id: u32) -> Option<Arc<[u8]>> {
        self.inner.read().unwrap().icons.get(&id).cloned()
    }

    /// Forgets the paths no process has run for `KEEP_IDLE`. Called on every
    /// sample; looks at most once every `SWEEP_EVERY`.
    pub(crate) fn sweep(&self) {
        self.sweep_at(Instant::now());
    }

    fn sweep_at(&self, now: Instant) {
        {
            let mut last = self.last_sweep.lock().unwrap();
            if last.is_some_and(|last| now.saturating_duration_since(last) < SWEEP_EVERY) {
                return;
            }
            *last = Some(now);
        }
        let mut inner = self.inner.write().unwrap();
        // Processes hold their executable's metadata while they run.
        inner.by_path.retain(|_, cached| cached.keep(now));
        inner.apps.retain(|_, cached| cached.keep(now));
        let referenced: HashSet<_> = inner
            .by_path
            .values()
            .filter_map(|entry| entry.meta.icon)
            .collect();
        // The two maps own one PNG reference each. Any other reference belongs
        // to a consumer, including the UI's paused or hidden snapshot.
        inner
            .icons
            .retain(|id, png| referenced.contains(id) || Arc::strong_count(png) > 2);
        let Inner {
            icons, icon_ids, ..
        } = &mut *inner;
        icon_ids.retain(|_, id| icons.contains_key(id));
        let reserve = inner.by_path.len() + 64;
        if inner.by_path.capacity() > reserve * 2 {
            inner.by_path.shrink_to(reserve);
        }
        let reserve = inner.apps.len() + 16;
        if inner.apps.capacity() > reserve * 2 {
            inner.apps.shrink_to(reserve);
        }
        let reserve = inner.icons.len() + 64;
        if inner.icons.capacity() > reserve * 2 {
            inner.icons.shrink_to(reserve);
            inner.icon_ids.shrink_to(reserve);
        }
    }

    fn insert(
        &self,
        path: &str,
        version: VersionInfo,
        icon_png: Option<Vec<u8>>,
        system_root: &str,
    ) {
        let key = path.to_lowercase();
        let mut inner = self.inner.write().unwrap();
        let icon = icon_png.and_then(|png| {
            let png: Arc<[u8]> = png.into();
            if let Some(&id) = inner.icon_ids.get(&png) {
                return Some(id);
            }
            let id = inner.next_icon;
            inner.next_icon = id.checked_add(1)?;
            inner.icons.insert(id, png.clone());
            inner.icon_ids.insert(png, id);
            Some(id)
        });
        let meta = FileMeta {
            windows_component: windows_component(path, &version, system_root),
            description: version.description.map(Into::into),
            company: version.company.map(Into::into),
            icon,
        };
        if let Some(bytes) = inner.pending.remove(&key) {
            inner.pending_bytes -= bytes;
        }
        inner.by_path.insert(
            key,
            Cached {
                meta: Arc::new(meta),
                idle_since: None,
            },
        );
    }

    fn insert_app(&self, identity: Identity, app: AppMeta) {
        let key = (identity.full_name, identity.aumid);
        let mut inner = self.inner.write().unwrap();
        inner.apps_pending.remove(&key);
        inner.apps.insert(
            key,
            Cached {
                meta: Arc::new(app),
                idle_since: None,
            },
        );
    }
}

fn worker(store: Weak<MetaStore>, rx: Receiver<String>) {
    // SHGetFileInfo requires COM on the calling thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let system_root = windows_directory();
    while let Ok(path) = rx.recv() {
        let version = version_info(&path);
        let icon = extract_icon_png(&path);
        match store.upgrade() {
            Some(store) => store.insert(&path, version, icon, &system_root),
            None => return,
        }
    }
}

fn app_worker(store: Weak<MetaStore>, rx: Receiver<Identity>) {
    // The shell's Applications folder requires COM on the calling thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    while let Ok(identity) = rx.recv() {
        let app = package::resolve(&identity);
        match store.upgrade() {
            Some(store) => store.insert_app(identity, app),
            None => return,
        }
    }
}

fn windows_directory() -> String {
    let mut directory = vec![0; 260];
    let mut len = unsafe { GetWindowsDirectoryW(Some(&mut directory)) } as usize;
    if len >= directory.len() && len < 32_768 {
        directory.resize(len + 1, 0);
        len = unsafe { GetWindowsDirectoryW(Some(&mut directory)) } as usize;
    }
    if len > 0 && len < directory.len() {
        String::from_utf16_lossy(&directory[..len])
    } else {
        String::new()
    }
}

/// A display category, not a signature check or permission to terminate a process.
fn windows_component(path: &str, version: &VersionInfo, system_root: &str) -> bool {
    if system_root.is_empty()
        || !version
            .company
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("Microsoft Corporation"))
        || !version.product_name.as_deref().is_some_and(|v| {
            v.replace('®', "")
                .eq_ignore_ascii_case("Microsoft Windows Operating System")
        })
    {
        return false;
    }
    let path = path.replace('/', "\\").to_lowercase();
    let root = format!("{}\\", system_root.trim_end_matches('\\').to_lowercase());
    let Some(relative) = path.strip_prefix(&root) else {
        return false;
    };
    // Third-party drivers and temp installers can also live under Windows.
    // Require OS version metadata and an OS component location together.
    match relative.split_once('\\') {
        None => !relative.is_empty(),
        Some((dir, _)) => matches!(
            dir,
            "system32" | "syswow64" | "systemapps" | "winsxs" | "servicing"
        ),
    }
}

/// String fields from an executable's version resource.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub description: Option<String>,
    pub company: Option<String>,
    pub product_name: Option<String>,
    pub file_version: Option<String>,
}

pub fn version_info(path: &str) -> VersionInfo {
    let wpath = wide(path);
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(wpath.as_ptr()), None) };
    if size == 0 {
        return VersionInfo::default();
    }
    let mut data = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(PCWSTR(wpath.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .is_err()
    {
        return VersionInfo::default();
    }

    // Prefer the first declared translation, then the common English fallbacks.
    let mut prefixes = Vec::new();
    unsafe {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let sub = wide(r"\VarFileInfo\Translation");
        if VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(sub.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && len >= 4
        {
            let pairs = std::slice::from_raw_parts(ptr as *const u16, len as usize / 2);
            for pair in pairs.as_chunks::<2>().0 {
                prefixes.push(format!(r"\StringFileInfo\{:04x}{:04x}\", pair[0], pair[1]));
            }
        }
    }
    prefixes.push(r"\StringFileInfo\040904b0\".into());
    prefixes.push(r"\StringFileInfo\040904e4\".into());

    let query = |field: &str| -> Option<String> {
        prefixes.iter().find_map(|prefix| unsafe {
            let sub = wide(&format!("{prefix}{field}"));
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            if !VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(sub.as_ptr()),
                &mut ptr,
                &mut len,
            )
            .as_bool()
                || len == 0
            {
                return None;
            }
            let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
            let s = String::from_utf16_lossy(s);
            let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
            (!s.is_empty()).then(|| s.to_owned())
        })
    };

    VersionInfo {
        description: query("FileDescription"),
        company: query("CompanyName"),
        product_name: query("ProductName"),
        file_version: query("FileVersion"),
    }
}

fn extract_icon_png(path: &str) -> Option<Vec<u8>> {
    let wpath = wide(path);
    let mut icon = HICON::default();
    let ok = if path.starts_with(r"\\?\") {
        // Shell item lookup rejects GLOBALROOT paths. Read the PE resource
        // for extended paths while keeping ordinary Shell icons unchanged.
        unsafe { SHDefExtractIconW(PCWSTR(wpath.as_ptr()), 0, 0, Some(&mut icon), None, 32) }
            .is_ok()
    } else {
        let mut info = SHFILEINFOW::default();
        let result = unsafe {
            SHGetFileInfoW(
                PCWSTR(wpath.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        icon = info.hIcon;
        result != 0
    };
    if icon.is_invalid() {
        return None;
    }
    let rgba = ok.then(|| unsafe { hicon_to_rgba(icon) }).flatten();
    unsafe {
        let _ = DestroyIcon(icon);
    }
    let (w, h, px) = rgba?;
    encode_png(w, h, &px)
}

/// Reads an icon's pixels as straight-alpha RGBA.
unsafe fn hicon_to_rgba(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut ii = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut ii) }.ok()?;
    let result = unsafe { read_icon_bitmaps(ii.hbmColor, ii.hbmMask) };
    unsafe {
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
    }
    result
}

unsafe fn read_icon_bitmaps(color: HBITMAP, mask: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    if color.is_invalid() {
        return None; // Monochrome icons are not worth supporting.
    }
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            color.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bm as *mut BITMAP).cast()),
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 256 || bm.bmHeight > 256 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let hdc = unsafe { GetDC(None) };
    let read = |bitmap: HBITMAP| -> Option<Vec<u8>> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut bmi = BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        };
        let lines = unsafe {
            GetDIBits(
                hdc,
                bitmap,
                0,
                h as u32,
                Some(px.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        (lines == h).then_some(px)
    };
    let mut px = read(color);
    if let Some(px) = px.as_mut()
        && !px.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
    {
        // Legacy icon without an alpha channel: derive alpha from the AND mask.
        match (!mask.is_invalid()).then(|| read(mask)).flatten() {
            Some(m) => {
                for (p, m) in px
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(m.as_chunks::<4>().0)
                {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            }
            None => px
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p[3] = 255),
        }
    }
    unsafe {
        ReleaseDC(None, hdc);
    }

    let mut px = px?;
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2); // BGRA -> RGBA
    }
    Some((w as u32, h as u32, px))
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(rgba.len() / 2);
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod category_tests {
    use super::*;

    #[test]
    fn stalled_resolvers_keep_the_request_backlog_bounded_and_retryable() {
        let (queue, pending) = mpsc::sync_channel(MAX_PENDING_PATHS);
        let (apps, app_pending) = mpsc::sync_channel(MAX_PENDING_APPS);
        let store = MetaStore {
            inner: RwLock::default(),
            queue,
            apps,
            last_sweep: Mutex::new(None),
        };
        let begin = Instant::now();
        for serial in 0..20_000 {
            store.get_or_request(&format!(r"c:\pressure\{serial}\app.exe"));
            store.app_or_request(&Identity {
                full_name: format!("package-{serial}").into(),
                aumid: "Package!App".into(),
            });
        }
        let inner = store.inner.read().unwrap();
        println!(
            "paths={} apps={} elapsed_ms={}",
            inner.pending.len(),
            inner.apps_pending.len(),
            begin.elapsed().as_millis()
        );
        assert!(inner.pending.len() <= 512 && inner.apps_pending.len() <= 64);
        drop(inner);
        let path = pending.try_recv().unwrap();
        store.insert(&path, VersionInfo::default(), None, "");
        let identity = app_pending.try_recv().unwrap();
        store.insert_app(identity, AppMeta::default());
        store.get_or_request(r"c:\pressure\retry.exe");
        let retry = Identity {
            full_name: "retry-package".into(),
            aumid: "Package!App".into(),
        };
        store.app_or_request(&retry);
        let inner = store.inner.read().unwrap();
        assert!(inner.pending.contains_key(r"c:\pressure\retry.exe"));
        assert!(inner.apps_pending.contains(&(retry.full_name, retry.aumid)));
    }

    #[test]
    fn pending_metadata_has_a_byte_budget_and_closed_workers_leave_no_pending_entries() {
        let (queue, pending) = mpsc::sync_channel(MAX_PENDING_PATHS);
        let (apps, app_pending) = mpsc::sync_channel(MAX_PENDING_APPS);
        let store = MetaStore {
            inner: RwLock::default(),
            queue,
            apps,
            last_sweep: Mutex::new(None),
        };
        for n in 0..100 {
            store.get_or_request(&format!("c:\\{n}\\{}.exe", "x".repeat(32 * 1024)));
        }
        let before = store.inner.read().unwrap().pending_bytes;
        assert!(before > MAX_PENDING_BYTES / 2 && before <= MAX_PENDING_BYTES);
        for path in pending.try_iter() {
            store.insert(&path, VersionInfo::default(), None, "");
        }
        assert_eq!(store.inner.read().unwrap().pending_bytes, 0);
        drop(pending);
        drop(app_pending);
        store.get_or_request(r"c:\closed-worker.exe");
        store.app_or_request(&Identity {
            full_name: "closed-worker".into(),
            aumid: "Package!App".into(),
        });
        let inner = store.inner.read().unwrap();
        assert!(inner.pending.is_empty() && inner.apps_pending.is_empty());
    }

    #[test]
    fn device_paths_need_a_win32_namespace_for_file_metadata() {
        use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
        use windows::Win32::System::Com::CoUninitialize;

        // Mirror the resolver worker's COM apartment for the Shell calls below.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .unwrap();
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let _apartment = Apartment;

        let root = windows_directory();
        let drive = wide(&root[..2]);
        let mut target = [0u16; 32768];
        assert!(unsafe { QueryDosDeviceW(PCWSTR(drive.as_ptr()), Some(&mut target)) } > 0);
        let native = format!(
            "{}{}\\explorer.exe",
            crate::util::from_wide(&target),
            &root[2..]
        );
        let win32 = format!(r"\\?\GLOBALROOT{native}");
        assert!(version_info(&native).description.is_none());
        assert!(extract_icon_png(&native).is_none());
        assert!(version_info(&win32).description.is_some());
        assert!(extract_icon_png(&win32).is_some());
        let ordinary = format!(r"{root}\explorer.exe");
        assert_eq!(
            version_info(&win32).description,
            version_info(&ordinary).description
        );
        assert!(extract_icon_png(&ordinary).is_some());
    }

    fn windows_version() -> VersionInfo {
        VersionInfo {
            company: Some("Microsoft Corporation".into()),
            product_name: Some("Microsoft® Windows® Operating System".into()),
            ..VersionInfo::default()
        }
    }

    #[test]
    fn os_metadata_requires_a_component_location_in_the_actual_windows_directory() {
        let version = windows_version();
        for path in [
            r"D:\WinNT\explorer.exe",
            r"d:\WINNT\System32\svchost.exe",
            r"D:\WinNT\SysWOW64\dllhost.exe",
            r"D:\WinNT\SystemApps\Shell\ShellExperienceHost.exe",
            r"D:\WinNT\WinSxS\component\worker.exe",
        ] {
            assert!(windows_component(path, &version, r"D:\WinNT"), "{path}");
        }
        for path in [
            r"C:\Windows\System32\svchost.exe",
            r"D:\WinNT-copy\System32\svchost.exe",
            r"D:\Apps\svchost.exe",
            r"D:\WinNT\Temp\setup.exe",
            r"D:\WinNT\Installer\setup.exe",
        ] {
            assert!(!windows_component(path, &version, r"D:\WinNT"), "{path}");
        }
        assert!(!windows_component(r"D:\WinNT\explorer.exe", &version, ""));
    }

    #[test]
    fn third_party_drivers_and_other_microsoft_products_are_not_windows_components() {
        let path = r"C:\Windows\System32\service.exe";
        for (company, product) in [
            ("Realtek", "Realtek HD Audio Universal Service"),
            ("Microsoft Corporation", "Microsoft Edge WebView2"),
            ("Microsoft Corporation", "Microsoft Office"),
            ("Example", "Microsoft Windows Operating System"),
        ] {
            let version = VersionInfo {
                company: Some(company.into()),
                product_name: Some(product.into()),
                ..VersionInfo::default()
            };
            assert!(!windows_component(path, &version, r"C:\Windows"));
        }
        assert!(!windows_component(
            path,
            &VersionInfo::default(),
            r"C:\Windows"
        ));
    }

    #[test]
    fn executables_with_the_same_icon_share_one_image() {
        let store = MetaStore::new();
        let add = |path: &str, png: &[u8]| {
            store.insert(path, VersionInfo::default(), Some(png.to_vec()), "");
            store.get_or_request(path).unwrap().icon.unwrap()
        };
        let first = add(r"C:\build\a\build-script-build.exe", &[1, 2, 3]);
        let second = add(r"C:\build\b\build-script-build.exe", &[1, 2, 3]);
        let other = add(r"C:\Apps\tool.exe", &[4, 5, 6]);
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(store.inner.read().unwrap().icons.len(), 2);
        assert_eq!(&*store.icon_png(other).unwrap(), &[4, 5, 6]);
    }

    #[test]
    fn paths_no_process_runs_are_forgotten_after_a_while() {
        let store = MetaStore::new();
        let cached = |path: &str| store.inner.read().unwrap().by_path.contains_key(path);
        for path in [r"c:\apps\running.exe", r"c:\build\once.exe"] {
            store.insert(path, VersionInfo::default(), None, "");
        }
        let running = store.get_or_request(r"C:\Apps\running.exe").unwrap();
        let start = Instant::now();
        store.sweep_at(start);
        assert!(cached(r"c:\build\once.exe"));
        // Not yet idle for long enough, and not swept again within a minute.
        store.sweep_at(start + KEEP_IDLE - SWEEP_EVERY / 2);
        store.sweep_at(start + KEEP_IDLE + SWEEP_EVERY / 4);
        assert!(cached(r"c:\build\once.exe"));
        let later = start + KEEP_IDLE + SWEEP_EVERY;
        store.sweep_at(later);
        assert!(!cached(r"c:\build\once.exe"));
        assert!(cached(r"c:\apps\running.exe"));
        // Once its last process exits, the running one goes the same way.
        drop(running);
        store.sweep_at(later + SWEEP_EVERY);
        store.sweep_at(later + SWEEP_EVERY * 2 + KEEP_IDLE);
        assert!(!cached(r"c:\apps\running.exe"));
    }

    #[test]
    fn unique_icons_and_package_versions_are_reclaimed_after_idle_expiry() {
        let store = MetaStore::new();
        let start = Instant::now();
        for id in 0u32..4096 {
            let mut png = vec![0; 4096];
            png[..4].copy_from_slice(&id.to_le_bytes());
            store.insert(
                &format!(r"c:\build\{id}\app.exe"),
                VersionInfo::default(),
                Some(png),
                "",
            );
            store.insert_app(
                Identity {
                    full_name: format!("TestPackage_{id}").into(),
                    aumid: "TestPackage!App".into(),
                },
                AppMeta::default(),
            );
        }
        store.sweep_at(start);
        store.sweep_at(start + KEEP_IDLE + SWEEP_EVERY);
        let inner = store.inner.read().unwrap();
        assert!(inner.by_path.is_empty());
        assert!(
            inner.icons.is_empty(),
            "{} orphaned icons remain after expiry",
            inner.icons.len()
        );
        assert!(
            inner.apps.is_empty(),
            "{} obsolete package versions remain after expiry",
            inner.apps.len()
        );
    }

    #[test]
    fn displayed_icons_survive_expiry_and_reclaimed_ids_are_never_reused() {
        let store = MetaStore::new();
        let path = r"c:\apps\paused.exe";
        store.insert(path, VersionInfo::default(), Some(vec![1, 2, 3]), "");
        let id = store.get_or_request(path).unwrap().icon.unwrap();
        let displayed = store.icon_png(id).unwrap();
        let start = Instant::now();
        store.sweep_at(start);
        store.sweep_at(start + KEEP_IDLE + SWEEP_EVERY);
        assert!(store.inner.read().unwrap().by_path.is_empty());
        assert!(Arc::ptr_eq(&displayed, &store.icon_png(id).unwrap()));
        drop(displayed);
        store.sweep_at(start + KEEP_IDLE + SWEEP_EVERY * 2);
        assert!(store.icon_png(id).is_none());
        store.insert(path, VersionInfo::default(), Some(vec![4, 5, 6]), "");
        assert_ne!(store.get_or_request(path).unwrap().icon, Some(id));
    }

    #[test]
    fn running_packaged_apps_keep_metadata_until_their_last_process_exits() {
        let store = MetaStore::new();
        let identity = Identity {
            full_name: "TestPackage_1".into(),
            aumid: "TestPackage!App".into(),
        };
        store.insert_app(identity.clone(), AppMeta::default());
        let running = store.app_or_request(&identity).unwrap();
        let start = Instant::now();
        store.sweep_at(start);
        store.sweep_at(start + KEEP_IDLE + SWEEP_EVERY);
        assert!(Arc::ptr_eq(
            &running,
            &store.app_or_request(&identity).unwrap()
        ));
        drop(running);
        let idle = start + KEEP_IDLE + SWEEP_EVERY * 2;
        store.sweep_at(idle);
        store.sweep_at(idle + KEEP_IDLE + SWEEP_EVERY);
        assert!(store.inner.read().unwrap().apps.is_empty());
    }

    #[test]
    fn thirty_days_of_metadata_churn_keep_only_live_icons() {
        let store = MetaStore::new();
        let start = Instant::now();
        for hour in 0..720 {
            for item in 0..128 {
                let serial: u32 = hour * 128 + item;
                store.insert(
                    &format!(r"c:\build\{serial}\app.exe"),
                    VersionInfo::default(),
                    Some(serial.to_le_bytes().to_vec()),
                    "",
                );
            }
            let now = start + Duration::from_secs(u64::from(hour) * 3600);
            store.sweep_at(now);
            store.sweep_at(now + KEEP_IDLE + SWEEP_EVERY);
            let inner = store.inner.read().unwrap();
            assert!(inner.by_path.is_empty());
            assert!(inner.icons.is_empty());
            assert!(inner.icon_ids.is_empty());
            assert!(inner.icons.capacity() <= 256);
        }
    }

    #[test]
    #[ignore = "live GDI resource soak; run alone with --test-threads=1"]
    fn resource_soak_icon_extraction() {
        use windows::Win32::System::Threading::{
            GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
            GetProcessHandleCount,
        };
        let counts = || unsafe {
            let process = GetCurrentProcess();
            let mut handles = 0;
            GetProcessHandleCount(process, &mut handles).unwrap();
            (
                handles,
                GetGuiResources(process, GR_GDIOBJECTS),
                GetGuiResources(process, GR_USEROBJECTS),
            )
        };
        // GLOBALROOT uses direct PE icon extraction, without Shell extensions.
        let path = format!(r"\\?\{}\explorer.exe", windows_directory());
        for _ in 0..10 {
            assert!(extract_icon_png(&path).is_some());
        }
        let before = counts();
        for batch in 0..8 {
            for _ in 0..250 {
                assert!(extract_icon_png(&path).is_some());
            }
            let after = counts();
            println!(
                "icon extraction batch={} handles={} gdi={} user={}",
                batch + 1,
                after.0,
                after.1,
                after.2
            );
            assert!(after.0 <= before.0 + 4);
            assert_eq!((after.1, after.2), (before.1, before.2));
        }
    }
}
