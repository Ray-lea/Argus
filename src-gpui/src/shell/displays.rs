//! The displays the desktop spans, for the taskbar monitor's choice of
//! taskbar: each display's monitor by what keeps it apart across restarts
//! and replugging (its device path), the name the monitor reports, whether
//! it is the primary display, and its area. Read at most once a second, and
//! again after the displays change; a test instance may name displays of
//! its own (`set_test`).
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

/// A display of the desktop.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Display {
    /// Its monitor's device path, the same after a restart; its GDI name
    /// (`\\.\DISPLAY2`) where Windows reports no path.
    pub key: String,
    /// The name the monitor reports ("DELL U2720Q"); empty without one.
    pub name: String,
    pub primary: bool,
    /// Its area on the virtual screen, physical pixels (left, top, right,
    /// bottom).
    pub rect: [i32; 4],
    /// Its `HMONITOR`, as a number; 0 for a test's display.
    #[serde(skip)]
    pub monitor: isize,
}

impl Display {
    pub fn width(&self) -> i32 {
        self.rect[2] - self.rect[0]
    }

    pub fn height(&self) -> i32 {
        self.rect[3] - self.rect[1]
    }
}

static READ: Mutex<Option<(Instant, Vec<Display>)>> = Mutex::new(None);
static TEST: Mutex<Option<Vec<Display>>> = Mutex::new(None);

/// The displays, the primary one first, then as Windows lists them.
pub fn displays() -> Vec<Display> {
    if let Some(test) = TEST.lock().unwrap().clone() {
        return test;
    }
    let mut read = READ.lock().unwrap();
    if let Some((at, list)) = read.as_ref()
        && at.elapsed() < Duration::from_secs(1)
    {
        return list.clone();
    }
    let list = read_displays();
    *read = Some((Instant::now(), list.clone()));
    list
}

/// The displays changed: the next look reads them again.
pub fn forget() {
    *READ.lock().unwrap() = None;
}

/// A test instance's displays in place of the system's; none to use the
/// system's again.
pub fn set_test(displays: Option<Vec<Display>>) {
    *TEST.lock().unwrap() = displays;
}

/// Whether a test instance names displays of its own.
pub fn testing() -> bool {
    TEST.lock().unwrap().is_some()
}

fn text(units: &[u16]) -> String {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

fn read_displays() -> Vec<Display> {
    let mut list: Vec<Display> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(gather), LPARAM((&raw mut list) as isize));
    }
    // The monitor on each display, by the GDI name of its source; a cloned
    // display shows one of its monitors.
    let targets = targets();
    for display in &mut list {
        if let Some((_, path, name)) = targets.iter().find(|(gdi, ..)| *gdi == display.key) {
            if !path.is_empty() {
                display.key = path.clone();
            }
            display.name = name.trim().to_owned();
        }
    }
    list.sort_by_key(|display| !display.primary);
    list
}

unsafe extern "system" fn gather(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = unsafe { &mut *(data.0 as *mut Vec<Display>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast()) }.as_bool() {
        let area = info.monitorInfo.rcMonitor;
        list.push(Display {
            key: text(&info.szDevice),
            name: String::new(),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            rect: [area.left, area.top, area.right, area.bottom],
            monitor: monitor.0 as isize,
        });
    }
    TRUE
}

/// Each active display path's source GDI name, its monitor's device path
/// and its monitor's name.
fn targets() -> Vec<(String, String, String)> {
    let (mut paths, mut modes) = (0u32, 0u32);
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes)
            != ERROR_SUCCESS
        {
            return Vec::new();
        }
        let mut path = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths,
            path.as_mut_ptr(),
            &mut modes,
            mode.as_mut_ptr(),
            None,
        ) != ERROR_SUCCESS
        {
            return Vec::new();
        }
        path.truncate(paths as usize);
        path.iter()
            .filter_map(|path| {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.sourceInfo.id,
                    },
                    ..Default::default()
                };
                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                        size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                        adapterId: path.targetInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    ..Default::default()
                };
                (DisplayConfigGetDeviceInfo(&mut source.header) == 0
                    && DisplayConfigGetDeviceInfo(&mut target.header) == 0)
                    .then(|| {
                        (
                            text(&source.viewGdiDeviceName),
                            text(&target.monitorDevicePath),
                            text(&target.monitorFriendlyDeviceName),
                        )
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::displays;

    #[test]
    fn the_primary_display_comes_first_and_each_has_a_key() {
        let list = displays();
        // A machine without a display (a service session) lists none.
        if list.is_empty() {
            return;
        }
        assert!(list[0].primary);
        assert_eq!(list.iter().filter(|display| display.primary).count(), 1);
        assert!(list.iter().all(|display| !display.key.is_empty()));
        assert!(
            list.iter()
                .all(|display| display.width() > 0 && display.height() > 0)
        );
    }
}
