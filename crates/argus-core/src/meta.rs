//! Per-executable metadata (version strings, icon), resolved off the sampling
//! thread and cached by path.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock, Weak};

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
use windows::core::PCWSTR;

use crate::util::wide;

/// Metadata shared by every process running the same executable.
#[derive(Debug, Default)]
pub struct FileMeta {
    pub description: Option<Arc<str>>,
    pub company: Option<Arc<str>>,
    /// Index into the icon table, served to the UI as a PNG.
    pub icon: Option<u32>,
}

#[derive(Default)]
struct Inner {
    by_path: HashMap<String, Arc<FileMeta>>,
    pending: HashSet<String>,
    icons: Vec<Arc<[u8]>>,
}

pub struct MetaStore {
    inner: RwLock<Inner>,
    queue: Sender<String>,
}

impl MetaStore {
    /// Creates the store and its background resolver thread.
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        let store = Arc::new(Self {
            inner: RwLock::default(),
            queue: tx,
        });
        let weak = Arc::downgrade(&store);
        std::thread::Builder::new()
            .name("argus-meta".into())
            .spawn(move || worker(weak, rx))
            .expect("spawn meta worker");
        store
    }

    /// Cached metadata for `path`; schedules resolution on a miss.
    pub fn get_or_request(&self, path: &str) -> Option<Arc<FileMeta>> {
        let key = path.to_lowercase();
        {
            let inner = self.inner.read().unwrap();
            if let Some(m) = inner.by_path.get(&key) {
                return Some(m.clone());
            }
            if inner.pending.contains(&key) {
                return None;
            }
        }
        let mut inner = self.inner.write().unwrap();
        if inner.pending.insert(key) {
            let _ = self.queue.send(path.to_owned());
        }
        None
    }

    /// PNG bytes of a previously resolved icon.
    pub fn icon_png(&self, id: u32) -> Option<Arc<[u8]>> {
        self.inner.read().unwrap().icons.get(id as usize).cloned()
    }

    fn insert(&self, path: &str, version: VersionInfo, icon_png: Option<Vec<u8>>) {
        let key = path.to_lowercase();
        let mut inner = self.inner.write().unwrap();
        let icon = icon_png.map(|png| {
            inner.icons.push(png.into());
            (inner.icons.len() - 1) as u32
        });
        let meta = FileMeta {
            description: version.description.map(Into::into),
            company: version.company.map(Into::into),
            icon,
        };
        inner.pending.remove(&key);
        inner.by_path.insert(key, Arc::new(meta));
    }
}

fn worker(store: Weak<MetaStore>, rx: Receiver<String>) {
    // SHGetFileInfo requires COM on the calling thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    while let Ok(path) = rx.recv() {
        let version = version_info(&path);
        let icon = extract_icon_png(&path);
        match store.upgrade() {
            Some(store) => store.insert(&path, version, icon),
            None => return,
        }
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
            for pair in pairs.chunks_exact(2) {
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
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let rgba = unsafe { hicon_to_rgba(info.hIcon) };
    unsafe {
        let _ = DestroyIcon(info.hIcon);
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
        && !px.chunks_exact(4).any(|p| p[3] != 0)
    {
        // Legacy icon without an alpha channel: derive alpha from the AND mask.
        match (!mask.is_invalid()).then(|| read(mask)).flatten() {
            Some(m) => {
                for (p, m) in px.chunks_exact_mut(4).zip(m.chunks_exact(4)) {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            }
            None => px.chunks_exact_mut(4).for_each(|p| p[3] = 255),
        }
    }
    unsafe {
        ReleaseDC(None, hdc);
    }

    let mut px = px?;
    for p in px.chunks_exact_mut(4) {
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
