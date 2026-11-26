//! Argus patch: per-draw batch parameters in reusable constant buffers,
//! bound by offset where the device supports D3D11.1 offsetting.
use super::*;

// D3D11.1 constant-buffer windows are aligned to 16 shader constants (256 bytes).
// Keep each buffer within the 4096-constant shader limit, regardless of scene size.
const CONSTANTS_PER_BATCH: usize = 16;
const BATCHES_PER_BUFFER: usize =
    D3D11_REQ_CONSTANT_BUFFER_ELEMENT_COUNT as usize / CONSTANTS_PER_BATCH;
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct BatchSlot {
    start_index: u32,
    padding: [u32; CONSTANTS_PER_BATCH * 4 - 1],
}
pub(super) struct BatchConstants {
    pub(super) legacy: Option<ID3D11Buffer>,
    context: Option<ID3D11DeviceContext1>,
    buffers: Vec<Option<ID3D11Buffer>>,
    indices: Vec<u32>,
    staging: Vec<BatchSlot>,
    pub(super) active: usize,
}
impl BatchConstants {
    pub(super) fn new(device: &ID3D11Device) -> Result<Self> {
        let legacy = create_constant_buffer::<BatchParams>(device)?;
        let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS::default();
        let offsetting = unsafe {
            device.CheckFeatureSupport(
                D3D11_FEATURE_D3D11_OPTIONS,
                (&mut options as *mut D3D11_FEATURE_DATA_D3D11_OPTIONS).cast(),
                std::mem::size_of_val(&options) as u32,
            )
        }
        .is_ok()
            && options.ConstantBufferOffsetting.as_bool();
        let context = if offsetting {
            unsafe { device.GetImmediateContext() }
                .ok()
                .and_then(|context| context.cast().ok())
        } else {
            None
        };
        if std::env::var_os("ARGUS_GPUI_DIAGNOSTICS").is_some() {
            println!(
                "{{\"event\":\"gpu-batch-constants\",\"offsetting\":{}}}",
                context.is_some()
            );
        }
        Ok(Self {
            legacy,
            context,
            buffers: Vec::new(),
            indices: Vec::new(),
            staging: Vec::new(),
            active: 0,
        })
    }
    pub(super) fn prepare(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        batches: &[PrimitiveBatch],
    ) -> Result<()> {
        if self.context.is_none() {
            return Ok(());
        }
        let indices = batches
            .iter()
            .map(|batch| match batch {
                PrimitiveBatch::Shadows(range)
                | PrimitiveBatch::Quads(range)
                | PrimitiveBatch::Underlines(range) => range.start as u32,
                PrimitiveBatch::MonochromeSprites { range, .. }
                | PrimitiveBatch::SubpixelSprites { range, .. }
                | PrimitiveBatch::PolychromeSprites { range, .. } => range.start as u32,
                PrimitiveBatch::Paths(_) | PrimitiveBatch::Surfaces(_) => 0,
            })
            .collect::<Vec<_>>();
        if self.indices == indices {
            return Ok(());
        }
        self.staging.clear();
        self.staging
            .extend(indices.iter().map(|&start_index| BatchSlot {
                start_index,
                padding: [0; CONSTANTS_PER_BATCH * 4 - 1],
            }));
        for (index, slots) in self.staging.chunks(BATCHES_PER_BUFFER).enumerate() {
            if self.buffers.len() <= index {
                self.buffers
                    .push(create_constant_buffer::<[BatchSlot; BATCHES_PER_BUFFER]>(
                        device,
                    )?);
            }
            update_buffer(
                context,
                self.buffers[index]
                    .as_ref()
                    .context("batch constant buffer missing")?,
                slots,
            )?;
        }
        self.indices = indices;
        Ok(())
    }
    pub(super) fn bind(&self, context: &ID3D11DeviceContext, first_instance: u32) -> Result<()> {
        if let Some(context1) = &self.context {
            anyhow::ensure!(
                self.indices.get(self.active) == Some(&first_instance),
                "batch constant index mismatch"
            );
            let buffer = &self.buffers[self.active / BATCHES_PER_BUFFER];
            let first = ((self.active % BATCHES_PER_BUFFER) * CONSTANTS_PER_BATCH) as u32;
            let count = CONSTANTS_PER_BATCH as u32;
            unsafe {
                context1.VSSetConstantBuffers1(
                    1,
                    1,
                    Some(buffer as *const Option<ID3D11Buffer>),
                    Some(&first),
                    Some(&count),
                );
            }
            Ok(())
        } else {
            update_batch_start(
                context,
                self.legacy
                    .as_ref()
                    .context("batch constant buffer missing")?,
                first_instance,
            )
        }
    }
}
