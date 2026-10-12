//! A thin wrapper over the Performance Data Helper (PDH) API for the counters
//! Windows publishes to every user: one query per consumer, collected once per
//! sample, with wildcard counters read back as (instance, value) pairs.

use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::core::PCWSTR;

use crate::util::AlignedBuf;

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

pub(crate) fn value(counter: PDH_HCOUNTER) -> Option<f64> {
    let mut v = PDH_FMT_COUNTERVALUE::default();
    (unsafe { PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut v) } == 0)
        .then_some(unsafe { v.Anonymous.doubleValue })
}

/// Every instance of a wildcard counter, as (instance name, value).
pub(crate) fn array(counter: PDH_HCOUNTER, buf: &mut AlignedBuf) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    each(counter, buf, |name, v| out.push((name.to_string(), v)));
    out
}

/// Calls `f` with every instance of a wildcard counter, without allocating per
/// instance (the name is only valid during the call).
pub(crate) fn each(counter: PDH_HCOUNTER, buf: &mut AlignedBuf, mut f: impl FnMut(&str, f64)) {
    let (mut size, mut count) = (0u32, 0u32);
    let status = unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
    };
    if status != PDH_MORE_DATA || size == 0 {
        return;
    }
    // The items are followed by the instance name strings they point into.
    if buf.len() < size as usize {
        buf.grow_to(size as usize);
    }
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    if unsafe {
        PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items))
    } != 0
    {
        return;
    }
    let mut name = String::new();
    for i in 0..count as usize {
        let item = unsafe { &*items.add(i) };
        // 0 and 1 are "valid" and "new data"; anything else failed this round.
        if item.FmtValue.CStatus > 1 {
            continue;
        }
        name.clear();
        let wide = unsafe { item.szName.as_wide() };
        name.extend(char::decode_utf16(wide.iter().copied()).map(|c| c.unwrap_or('\u{FFFD}')));
        f(&name, unsafe { item.FmtValue.Anonymous.doubleValue });
    }
}
