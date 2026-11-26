//! Argus patch: the full-window MSAA and resolve targets of vector paths,
//! allocated at the first path batch and released while unused.
use super::*;

pub(super) struct PathTargets {
    pub(super) resolved: ID3D11Texture2D,
    pub(super) srv: Option<ID3D11ShaderResourceView>,
    pub(super) msaa: ID3D11Texture2D,
    pub(super) rtv: Option<ID3D11RenderTargetView>,
}

impl PathTargets {
    pub(super) fn new(device: &ID3D11Device, width: u32, height: u32) -> Result<Self> {
        let (resolved, srv) = create_path_intermediate_texture(device, width, height)?;
        let (msaa, rtv) = create_path_intermediate_msaa_texture_and_view(device, width, height)?;
        Ok(Self {
            resolved,
            srv,
            msaa,
            rtv,
        })
    }
}

pub(super) fn path_target_trace(action: &str, width: u32, height: u32) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some()) {
        let bytes =
            u64::from(width) * u64::from(height) * 4 * u64::from(1 + PATH_MULTISAMPLE_COUNT);
        println!("{{\"event\":\"gpu-path-targets\",\"action\":\"{action}\",\"bytes\":{bytes}}}");
    }
}

impl DirectXRenderer {
    pub(super) fn release_path_targets(&mut self) {
        if let Some(resources) = self.resources.as_mut()
            && resources.paths.take().is_some()
            && let Some(devices) = &self.devices
        {
            // The path sprite's SRV may still be bound after its last draw.
            unsafe {
                devices
                    .device_context
                    .VSSetShaderResources(0, Some(&[None]));
                devices
                    .device_context
                    .PSSetShaderResources(0, Some(&[None]));
            }
            path_target_trace("release", self.width, self.height);
        }
    }
}
