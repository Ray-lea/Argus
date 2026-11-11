//! Development probes for separating Windows GPU queries from vendor SDKs.
//! Each entry uses the ordinary collector implementation and no GUI or service.
use crate::diagnostics::startup_phase;

/// Enumerate/sample Windows GPU data without loading NVML, ADL or IGCL.
pub fn windows_gpu() -> usize {
    let mut tracker = startup_phase("probe.windows-gpu.construct", || {
        super::gpu::GpuTracker::new_with_vendor(false)
    });
    let count = startup_phase("probe.windows-gpu.sample", || tracker.sample().len());
    startup_phase("probe.windows-gpu.close", || drop(tracker));
    count
}

/// Load and sample NVML without D3DKMT enumeration, PDH or window creation.
pub fn nvidia() -> Option<usize> {
    startup_phase("probe.nvml", super::nvml::Nvml::probe_once)
}
