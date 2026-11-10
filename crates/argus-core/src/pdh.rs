//! A thin wrapper over the Performance Data Helper (PDH) API for the counters
//! Windows publishes to every user: one query per consumer, collected once per
//! sample, with wildcard counters read back as (instance, value) pairs.

use windows::Win32::System::Performance::{
    PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW,
};
use windows::core::PCWSTR;

use crate::util::AlignedBuf;

// pdh.h; this scaling flag is omitted from the generated Windows bindings.
const PDH_FMT_NOCAP100: PDH_FMT = PDH_FMT(0x0000_8000);

pub(crate) struct Query(PDH_HQUERY);

// PDH handles are process-wide; each query is used from one thread at a time.
unsafe impl Send for Query {}

impl Query {
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        (unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0).then_some(Self(query))
    }

    /// Adds a counter by its English path; None if this machine lacks it.
    pub fn add(&self, path: PCWSTR) -> Option<PDH_HCOUNTER> {
        let mut counter = PDH_HCOUNTER::default();
        (unsafe { PdhAddEnglishCounterW(self.0, path, 0, &mut counter) } == 0).then_some(counter)
    }

    /// Takes a new reading of every counter; false if PDH failed.
    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.0) == 0 }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.0) };
    }
}

/// Every instance of a wildcard counter, as (instance name, value).
pub(crate) fn array(counter: PDH_HCOUNTER, buf: &mut AlignedBuf) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    each(counter, buf, |name, v| out.push((name.to_string(), v)));
    out
}

/// CPU boost performance may legitimately exceed 100 percent.
pub(crate) fn array_uncapped(counter: PDH_HCOUNTER, buf: &mut AlignedBuf) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    formatted_array(
        counter,
        buf,
        PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100.0),
        |name, value| {
            if let Some(value) = value {
                out.push((name.to_owned(), value));
            }
        },
    );
    out
}

/// Calls `f` with every instance of a wildcard counter, without allocating per
/// instance (the name is only valid during the call).
pub(crate) fn each(counter: PDH_HCOUNTER, buf: &mut AlignedBuf, mut f: impl FnMut(&str, f64)) {
    each_value(counter, buf, |name, value| {
        if let Some(value) = value {
            f(name, value);
        }
    });
}

/// Includes unavailable instances so consumers can distinguish missing values from zero.
pub(crate) fn each_value(
    counter: PDH_HCOUNTER,
    buf: &mut AlignedBuf,
    f: impl FnMut(&str, Option<f64>),
) -> bool {
    formatted_array(counter, buf, PDH_FMT_DOUBLE, f)
}

fn formatted_array(
    counter: PDH_HCOUNTER,
    buf: &mut AlignedBuf,
    format: PDH_FMT,
    mut f: impl FnMut(&str, Option<f64>),
) -> bool {
    let (mut size, mut count) = (0u32, 0u32);
    let status =
        unsafe { PdhGetFormattedCounterArrayW(counter, format, &mut size, &mut count, None) };
    if status != PDH_MORE_DATA || size == 0 {
        return false;
    }
    // The items are followed by the instance name strings they point into.
    if buf.len() < size as usize {
        buf.grow_to(size as usize);
    }
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    if unsafe { PdhGetFormattedCounterArrayW(counter, format, &mut size, &mut count, Some(items)) }
        != 0
    {
        return false;
    }
    let mut name = String::new();
    for i in 0..count as usize {
        let item = unsafe { &*items.add(i) };
        name.clear();
        let wide = unsafe { item.szName.as_wide() };
        name.extend(char::decode_utf16(wide.iter().copied()).map(|c| c.unwrap_or('\u{FFFD}')));
        // 0 and 1 are valid/new data; other states retain the instance as unavailable.
        let value = if item.FmtValue.CStatus <= 1 {
            Some(unsafe { item.FmtValue.Anonymous.doubleValue })
        } else {
            None
        };
        f(&name, value);
    }
    true
}
