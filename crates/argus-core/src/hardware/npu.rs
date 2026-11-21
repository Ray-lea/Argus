//! NPUs via DXCore hardware-type attributes and D3DKMT scheduler node statistics.

use std::collections::HashSet;
use std::ffi::c_void;
use std::time::Instant;

use serde::Serialize;
use windows::Win32::Foundation::{E_INVALIDARG, LUID};
use windows::Win32::Graphics::DXCore::{
    DXCoreAdapterProperty, DriverDescription, IDXCoreAdapter, IDXCoreAdapterFactory,
    IDXCoreAdapterList, InstanceLuid, IsHardware,
};
use windows::core::{GUID, HRESULT, Interface, Type, s, w};

use super::gpu::{NodeCounter, adapter_statistics, luid_key, node_running_time, reconcile};
use super::query::Retry;
use super::vendor_gpu::Library;
use crate::diagnostics::{startup_native, startup_phase};

// From dxcore_interface.h. windows 0.62.2 omits the hardware-type attributes.
const DXCORE_HARDWARE_TYPE_ATTRIBUTE_GPU: GUID =
    GUID::from_u128(0xb69eb219_3ded_4464_979f_a00bd4687006);
const DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU: GUID =
    GUID::from_u128(0xd46140c4_add7_451b_9e56_06fe8c3b58ed);
const MAX_ADAPTERS: u32 = 64;
const MAX_NODES: u32 = 64;

type CreateAdapterFactory = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpuStats {
    /// Adapter LUID as a string.
    pub id: String,
    pub name: String,
    /// Busiest node, 0–100; `None` without a valid rate window or node counters.
    pub util: Option<f32>,
}

struct Adapter {
    dxcore: IDXCoreAdapter,
    luid: LUID,
    id: String,
    name: String,
    prev_running: Vec<NodeCounter>,
    nodes_query: Retry,
}

struct DxCore {
    list: Option<IDXCoreAdapterList>,
    factory: IDXCoreAdapterFactory,
    list_retry: Retry,
    _library: Library,
}

#[derive(Default)]
pub(crate) struct NpuTracker {
    // Fields drop in declaration order: adapters, then list and factory, then the DLL.
    adapters: Vec<Adapter>,
    dxcore: Option<DxCore>,
}

// DXCore interfaces are free-threaded. The module handle can be released on any thread.
unsafe impl Send for NpuTracker {}

impl NpuTracker {
    pub fn new() -> Self {
        startup_phase("npu.enumerate", Self::init).unwrap_or_default()
    }

    fn init() -> Option<Self> {
        let library = Library::load(w!("dxcore.dll"))?;
        let create: CreateAdapterFactory =
            unsafe { library.export(s!("DXCoreCreateAdapterFactory")) }?;
        let factory = dxcore_call("npu.dxcore-factory", 0, || unsafe {
            let mut raw = std::ptr::null_mut();
            create(&IDXCoreAdapterFactory::IID, &mut raw)
                .and_then(|| IDXCoreAdapterFactory::from_abi(raw))
        })
        .ok()?;
        let mut list_retry = Retry::default();
        let mut unsupported = false;
        let list = list_retry.query(Instant::now(), || match npu_list(&factory) {
            // Downlevel DXCore does not recognize the NPU hardware-type attribute.
            Err(E_INVALIDARG) => {
                unsupported = true;
                None
            }
            result => result.ok(),
        });
        if unsupported {
            return None;
        }
        Some(Self {
            adapters: list.as_ref().map(enumerate_list).unwrap_or_default(),
            dxcore: Some(DxCore {
                list,
                factory,
                list_retry,
                _library: library,
            }),
        })
    }

    pub(crate) fn luids(&self) -> HashSet<u64> {
        self.adapters.iter().map(|a| luid_key(a.luid)).collect()
    }

    pub(crate) fn refresh(&mut self) {
        self.refresh_at(Instant::now(), true);
    }

    fn refresh_at(&mut self, now: Instant, force: bool) {
        let Some(dxcore) = &mut self.dxcore else {
            return;
        };
        // Remove invalid adapters even while list creation is backing off.
        let before = self.adapters.len();
        self.adapters.retain(|a| unsafe { a.dxcore.IsValid() });
        if !force
            && before == self.adapters.len()
            && dxcore
                .list
                .as_ref()
                .is_some_and(|list| unsafe { !list.IsStale() })
        {
            return;
        }
        if force {
            dxcore.list_retry = Retry::default();
        }
        dxcore.list = dxcore
            .list_retry
            .query(now, || npu_list(&dxcore.factory).ok());
        let Some(list) = &dxcore.list else { return };
        let previous = std::mem::take(&mut self.adapters);
        self.adapters = reconcile(previous, enumerate_list(list), |a| luid_key(a.luid)).0;
        log::info!(target: "argus_diagnostics", "npu adapters enumerated again; listed={}", self.adapters.len());
    }

    pub fn sample(&mut self) -> Vec<NpuStats> {
        let now = Instant::now();
        self.refresh_at(now, false);
        self.adapters.iter_mut().map(|a| a.sample(now)).collect()
    }
}

// Keep first-call HRESULT diagnostics when consuming projected COM results.
fn dxcore_call<T>(
    phase: &'static str,
    instance: u64,
    call: impl FnOnce() -> windows::core::Result<T>,
) -> Result<T, HRESULT> {
    let mut value = None;
    let status = startup_native(phase, instance, 0, || match call() {
        Ok(result) => {
            value = Some(result);
            0
        }
        Err(error) => error.code().0,
    });
    value.ok_or(HRESULT(status))
}

fn npu_list(factory: &IDXCoreAdapterFactory) -> Result<IDXCoreAdapterList, HRESULT> {
    dxcore_call("npu.dxcore-list", 0, || unsafe {
        factory.CreateAdapterList(&[DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU])
    })
}

fn enumerate_list(list: &IDXCoreAdapterList) -> Vec<Adapter> {
    let mut seen = HashSet::new();
    (0..unsafe { list.GetAdapterCount() }.min(MAX_ADAPTERS))
        .filter_map(|i| {
            dxcore_call("npu.dxcore-adapter", u64::from(i), || unsafe {
                list.GetAdapter(i)
            })
            .ok()
        })
        .filter_map(Adapter::describe)
        .filter(|a| seen.insert(luid_key(a.luid)))
        .collect()
}

impl Adapter {
    fn describe(dxcore: IDXCoreAdapter) -> Option<Self> {
        let luid = unsafe {
            if !dxcore.IsValid()
                || !dxcore.IsAttributeSupported(&DXCORE_HARDWARE_TYPE_ATTRIBUTE_NPU)
                || dxcore.IsAttributeSupported(&DXCORE_HARDWARE_TYPE_ATTRIBUTE_GPU)
                || property::<u8>(&dxcore, IsHardware)? == 0
            {
                return None;
            }
            property::<LUID>(&dxcore, InstanceLuid)?
        };
        let key = luid_key(luid);
        if key == 0 {
            return None;
        }
        let name = description(&dxcore).unwrap_or_else(|| "NPU".into());
        let mut nodes_query = Retry::default();
        let nodes = nodes_query
            .query(Instant::now(), || adapter_node_count(luid))
            .unwrap_or(0);
        log::info!(target: "argus_startup", "phase=npu.describe; instance={key}; nodes={nodes}");
        Some(Self {
            dxcore,
            luid,
            id: key.to_string(),
            name,
            prev_running: (0..nodes).map(|_| NodeCounter::default()).collect(),
            nodes_query,
        })
    }

    fn sample(&mut self, now: Instant) -> NpuStats {
        if self.prev_running.is_empty()
            && let Some(nodes) = self
                .nodes_query
                .query(now, || adapter_node_count(self.luid))
        {
            self.prev_running.resize_with(nodes, NodeCounter::default);
        }
        let mut any_ok = false;
        let util = self
            .prev_running
            .iter_mut()
            .enumerate()
            .filter_map(|(node, prev)| {
                let running = node_running_time("npu.node-statistics", self.luid, node as u32);
                any_ok |= running.is_some();
                prev.sample(running, Instant::now())
            })
            .reduce(f32::max);
        if !any_ok {
            self.prev_running.clear();
        }
        NpuStats {
            id: self.id.clone(),
            name: self.name.clone(),
            util,
        }
    }
}

// The caller must pair the property with its SDK buffer type.
unsafe fn property<T: Copy + Default>(
    adapter: &IDXCoreAdapter,
    prop: DXCoreAdapterProperty,
) -> Option<T> {
    let mut value = T::default();
    unsafe { adapter.GetProperty(prop, size_of::<T>(), (&raw mut value).cast()) }.ok()?;
    Some(value)
}

fn description(adapter: &IDXCoreAdapter) -> Option<String> {
    let size = unsafe { adapter.GetPropertySize(DriverDescription) }.ok()?;
    if !(1..=4096).contains(&size) {
        return None;
    }
    let mut bytes = vec![0u8; size];
    unsafe { adapter.GetProperty(DriverDescription, bytes.len(), bytes.as_mut_ptr().cast()) }
        .ok()?;
    Some(adapter_name(&bytes))
}

fn adapter_node_count(luid: LUID) -> Option<usize> {
    let (nodes, _) = adapter_statistics("npu.adapter-statistics", luid)?;
    (nodes > 0).then_some(nodes.min(MAX_NODES) as usize)
}

fn adapter_name(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "NPU".into()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_snapshot_distinguishes_idle_from_unavailable() {
        for util in [None, Some(0.0)] {
            let stats = NpuStats {
                id: "42".into(),
                name: "NPU".into(),
                util,
            };
            assert_eq!(
                serde_json::to_value(stats).unwrap(),
                serde_json::json!({"id":"42", "name":"NPU", "util":util})
            );
        }
        let hw = super::super::Hardware::default();
        assert_eq!(
            serde_json::to_value(hw).unwrap()["npus"],
            serde_json::json!([])
        );
    }

    #[test]
    fn driver_description_trims_nul_and_whitespace_with_fallback() {
        assert_eq!(
            adapter_name(b"  Intel(R) AI Boost \0trailing"),
            "Intel(R) AI Boost"
        );
        assert_eq!(adapter_name(b"   \0"), "NPU");
    }
}
