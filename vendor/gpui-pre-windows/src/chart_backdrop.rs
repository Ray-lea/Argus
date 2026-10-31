//! Find strips of horizontally identical backdrop pixels before caching a moving chart.
use super::*;
use std::collections::BTreeMap;
use std::task::Poll;

const MAX_STRIPS: usize = 48;
type ColumnRuns = BTreeMap<usize, Vec<RECT>>;

#[derive(Clone, Copy)]
pub(super) struct Strip {
    pub clip: RECT,
    pub first: i32,
    pub width: u32,
    pub top: i32,
}

impl Strip {
    pub fn new(clip: RECT, bounds: Bounds<ScaledPixels>) -> Option<Self> {
        let left = bounds.left().0;
        // Leave room for about two samples of a 60-sample chart. Larger moves
        // use the normal scene until the next cache is ready.
        let guard = 4 * (bounds.size.width.0 / 32.).ceil().clamp(8., 128.) as i32 + 2;
        let first = (((clip.left as f32 - left) * 4.).floor() as i32).checked_sub(guard)?;
        let end = (((clip.right as f32 - left) * 4.).ceil() as i32).checked_add(guard)?;
        let width = u32::try_from(end.checked_sub(first)?).ok()?;
        (width > 0 && width <= 16384).then_some(Self {
            clip,
            first,
            width,
            top: (bounds.top().0 - 0.5).ceil() as i32,
        })
    }

    pub fn fits(self, bounds: Bounds<ScaledPixels>) -> bool {
        let left = bounds.left().0;
        self.top == (bounds.top().0 - 0.5).ceil() as i32
            && (self.clip.left as f32 - left) * 4. >= (self.first + 2) as f32
            && (self.clip.right as f32 - left) * 4. <= self.first as f32 + self.width as f32 - 2.
    }
}

pub(super) struct Scan {
    pub key: FrameKey,
    pub regions: Vec<(usize, RECT)>,
    readback: ID3D11Texture2D,
}

impl Scan {
    pub fn start(
        key: FrameKey,
        regions: Vec<(usize, RECT)>,
        devices: &DirectXRendererDevices,
        filter: &Filter,
        prefix: &ID3D11ShaderResourceView,
    ) -> Result<Self> {
        let width = regions
            .iter()
            .map(|(_, r)| (r.right - r.left) as u32)
            .max()
            .context("empty chart scan")?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: regions.len() as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let ctx = &devices.device_context;
        unsafe {
            let mut texture = None;
            devices
                .device
                .CreateTexture2D(&desc, None, Some(&mut texture))?;
            let texture = texture.context("chart column flags")?;
            let mut rtv = None;
            devices
                .device
                .CreateRenderTargetView(&texture, None, Some(&mut rtv))?;
            let mut readback = None;
            devices.device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    ..desc
                },
                None,
                Some(&mut readback),
            )?;
            let readback = readback.context("chart column readback")?;
            ctx.OMSetRenderTargets(Some(&[rtv]), None);
            ctx.OMSetBlendState(None, None, u32::MAX);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.VSSetShader(&filter.vertex, None);
            ctx.PSSetShader(&filter.columns, None);
            ctx.PSSetShaderResources(2, Some(&[Some(prefix.clone())]));
            ctx.PSSetConstantBuffers(2, Some(&[Some(filter.params.clone())]));
            let draw: Result<()> = (|| {
                for (index, (_, r)) in regions.iter().enumerate() {
                    let mut params = [0i32; 12];
                    params[..4].copy_from_slice(&[
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                    ]);
                    update_buffer(ctx, &filter.params, &params)?;
                    ctx.RSSetViewports(Some(&[viewport(
                        0.,
                        index as f32,
                        (r.right - r.left) as u32,
                        1,
                    )]));
                    ctx.Draw(4, 0);
                }
                Ok(())
            })();
            ctx.PSSetShaderResources(2, Some(&[None]));
            ctx.OMSetRenderTargets(None, None);
            draw?;
            ctx.CopyResource(&readback, &texture);
            Ok(Self {
                key,
                regions,
                readback,
            })
        }
    }

    pub fn poll(&self, ctx: &ID3D11DeviceContext) -> Result<Poll<Option<ColumnRuns>>> {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            match ctx.Map(
                &self.readback,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(Poll::Pending),
                Err(e) => return Err(e.into()),
            }
            let result = self
                .regions
                .iter()
                .enumerate()
                .map(|(row, &(index, r))| {
                    // The GPU writes exactly one R8 flag per column in this row.
                    let data = std::slice::from_raw_parts(
                        mapped
                            .pData
                            .cast::<u8>()
                            .add(row * mapped.RowPitch as usize),
                        (r.right - r.left) as usize,
                    );
                    Some((index, strips(r, data)?))
                })
                .collect();
            ctx.Unmap(&self.readback, 0);
            Ok(Poll::Ready(result))
        }
    }
}

fn strips(rect: RECT, flags: &[u8]) -> Option<Vec<RECT>> {
    let mut start = rect.left;
    let mut result = Vec::new();
    for (i, &flag) in flags.iter().enumerate() {
        if flag > 1 {
            if diagnostics() {
                println!(
                    "{{\"event\":\"gpu-chart-backdrop-limit\",\"reason\":\"translucent-prefix\",\"columns\":{},\"height\":{}}}",
                    flags.iter().filter(|&&f| f > 1).count(),
                    rect.bottom - rect.top
                );
            }
            return None;
        }
        if i != 0 && flag == 1 {
            let x = rect.left + i as i32;
            result.push(RECT {
                left: start,
                right: x,
                ..rect
            });
            if result.len() >= MAX_STRIPS {
                if diagnostics() {
                    println!(
                        "{{\"event\":\"gpu-chart-backdrop-limit\",\"reason\":\"strip-count\",\"limit\":{MAX_STRIPS}}}"
                    );
                }
                return None;
            }
            start = x;
        }
    }
    result.push(RECT {
        left: start,
        ..rect
    });
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{MAX_STRIPS, Strip, strips};
    use gpui::{Bounds, ScaledPixels, point, size};
    use windows::Win32::Foundation::RECT;
    #[test]
    fn a_cached_strip_accepts_small_scrolls_and_keeps_its_screen_rows() {
        let mut bounds = Bounds::new(
            point(ScaledPixels(19.875), ScaledPixels(12.25)),
            size(ScaledPixels(800.), ScaledPixels(250.)),
        );
        let strip = Strip::new(
            RECT {
                left: 80,
                right: 82,
                top: 140,
                bottom: 260,
            },
            bounds,
        )
        .unwrap();
        assert!(strip.fits(bounds));
        bounds.origin.x.0 -= 15.;
        assert!(strip.fits(bounds));
        bounds.origin.y.0 += 1.;
        assert!(!strip.fits(bounds));
        bounds.origin.y.0 -= 1.;
        bounds.origin.x.0 -= 50.;
        assert!(!strip.fits(bounds));
    }

    #[test]
    fn strips_cover_the_same_columns_and_reject_translucent_or_complex_backdrops() {
        let rect = RECT {
            left: 10,
            right: 16,
            top: 3,
            bottom: 12,
        };
        let result = strips(rect, &[0, 0, 1, 1, 0, 0]).unwrap();
        assert_eq!(
            result.iter().map(|r| (r.left, r.right)).collect::<Vec<_>>(),
            [(10, 12), (12, 13), (13, 16)]
        );
        assert!(strips(rect, &[0, 0, 2, 0, 0, 0]).is_none());
        assert!(
            strips(
                RECT {
                    right: 10 + MAX_STRIPS as i32 + 1,
                    ..rect
                },
                &[1; MAX_STRIPS + 1]
            )
            .is_none()
        );
    }
}
