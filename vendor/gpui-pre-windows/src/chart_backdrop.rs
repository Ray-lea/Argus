//! Find strips of horizontally identical backdrop pixels before caching a moving chart.
use super::*;
use std::collections::BTreeMap;
use std::task::Poll;

const MAX_STRIPS: usize = 48;
const MAX_SHEET_WIDTH: u32 = 16384;
type ColumnRuns = BTreeMap<usize, Columns>;

pub(super) struct Columns {
    pub rect: RECT,
    pub runs: Vec<RECT>,
    exceptions: Vec<RECT>,
}

#[derive(Default)]
pub(super) struct Plan {
    pub strips: Vec<Strip>,
    pub corrections: Vec<Strip>,
}

impl Plan {
    pub fn clips(&self) -> usize {
        self.strips.len() + usize::from(!self.corrections.is_empty())
    }

    pub fn bytes(&self, height: u64) -> u64 {
        let packed: u64 = self.strips.iter().map(|s| u64::from(s.width)).sum();
        let mut bytes = packed * height * 4;
        if let Some(base) = self.strips.first() {
            // Reserve padded updates, including tiles straddled by a column.
            for s in &self.corrections {
                let start = (s.clip.left - base.clip.left) as u64 * u64::from(s.stride);
                let end = (start + u64::from(s.width)).next_multiple_of(128);
                bytes += (end - start / 128 * 128) * height.next_multiple_of(64) * 4;
            }
        }
        bytes
    }
}

fn reference_x(rect: RECT) -> i32 {
    rect.left + (rect.right - rect.left) / 11
}

#[derive(Clone, Copy)]
pub(super) struct Strip {
    pub clip: RECT,
    pub first: i32,
    pub width: u32,
    pub top: i32,
    pub column: i32,
    /// Horizontal phases per device pixel in the filtered bitmap.
    pub samples: u32,
    /// Samples stored for each fixed screen column; zero for a uniform span.
    pub stride: u32,
}

impl Strip {
    pub fn new(clip: RECT, bounds: Bounds<ScaledPixels>, samples: u32) -> Option<Self> {
        let left = bounds.left().0;
        // Leave room for about two samples of a 60-sample chart. Larger moves
        // use the normal scene until the next cache is ready.
        let guard = samples as i32 * (bounds.size.width.0 / 32.).ceil().clamp(8., 128.) as i32 + 2;
        let first =
            (((clip.left as f32 - left) * samples as f32).floor() as i32).checked_sub(guard)?;
        let end =
            (((clip.right as f32 - left) * samples as f32).ceil() as i32).checked_add(guard)?;
        let width = u32::try_from(end.checked_sub(first)?).ok()?;
        (width > 0 && width <= MAX_SHEET_WIDTH).then_some(Self {
            clip,
            first,
            width,
            top: (bounds.top().0 - 0.5).ceil() as i32,
            column: clip.left,
            samples,
            stride: 0,
        })
    }

    pub fn phase(self, left: f32) -> f32 {
        (self.clip.left as f32 + 0.5 - left) * 8. - self.first as f32
    }

    pub fn fits(self, bounds: Bounds<ScaledPixels>) -> bool {
        let left = bounds.left().0;
        self.top == (bounds.top().0 - 0.5).ceil() as i32
            && if self.stride > 0 {
                (1.0..=self.stride as f32 - 2.).contains(&self.phase(left))
            } else {
                (self.clip.left as f32 - left) * self.samples as f32 >= (self.first + 2) as f32
                    && (self.clip.right as f32 - left) * self.samples as f32
                        <= self.first as f32 + self.width as f32 - 2.
            }
    }
}

fn append_lanes(rect: RECT, bounds: Bounds<ScaledPixels>, result: &mut Vec<Strip>) -> Option<()> {
    // Keep a sample interval ahead and a smaller margin for corrections.
    // The power-of-two stride keeps the fixed offset exact at high DPI.
    let travel = 8 * (bounds.size.width.0 / 48.).ceil().clamp(2., 128.) as u32;
    let stride = (travel + 16).next_power_of_two();
    let guard = (stride / 4).min(stride - travel - 2) as i32;
    let anchor = bounds.left().0.round();
    let mut left = rect.left;
    while left < rect.right {
        let right = rect.right.min(left + (MAX_SHEET_WIDTH / stride) as i32);
        let source = ((left as f32 + 0.5 - anchor) * 8.) as i32;
        result.push(Strip {
            clip: RECT {
                left,
                right,
                ..rect
            },
            first: source.checked_sub(guard)?,
            width: (right - left) as u32 * stride,
            top: (bounds.top().0 - 0.5).ceil() as i32,
            column: left,
            samples: 8,
            stride,
        });
        left = right;
    }
    Some(())
}

/// Dense glyph edges share a clip. Each screen column gets its own lane of
/// eighth-pixel curve phases over that column's actual background.
pub(super) fn pack(
    runs: &[RECT],
    bounds: Bounds<ScaledPixels>,
    samples: u32,
) -> Option<Vec<Strip>> {
    let mut result = Vec::new();
    let mut remaining = runs;
    while let Some(first) = remaining.first() {
        let narrow = remaining
            .iter()
            .take_while(|r| r.right - r.left < 8)
            .count();
        if narrow < 2 {
            result.push(Strip::new(*first, bounds, samples)?);
            remaining = &remaining[1..];
        } else {
            append_lanes(
                RECT {
                    right: remaining[narrow - 1].right,
                    ..*first
                },
                bounds,
                &mut result,
            )?;
            remaining = &remaining[narrow..];
        }
        if result.len() > MAX_STRIPS {
            return None;
        }
    }
    Some(result)
}

impl Columns {
    /// One moving opaque chart covers the common background. A virtual
    /// surface stores only the fixed columns whose actual pixels differ.
    pub fn sparse(&self, bounds: Bounds<ScaledPixels>, samples: u32) -> Option<Plan> {
        let columns: i32 = self.exceptions.iter().map(|r| r.right - r.left).sum();
        if columns == 0 || columns * 4 > self.rect.right - self.rect.left {
            return None;
        }
        let mut base = Strip::new(self.rect, bounds, samples)?;
        base.column = reference_x(self.rect);
        let mut corrections = Vec::new();
        for &rect in &self.exceptions {
            append_lanes(rect, bounds, &mut corrections)?;
        }
        Some(Plan {
            strips: vec![base],
            corrections,
        })
    }
}

/// Pack adjacent strips at their original sampling density. A plain
/// transparent sprite uses one sheet with no backdrop strips.
pub(super) fn sheets(strips: &[Strip]) -> Vec<&[Strip]> {
    let mut sheets = Vec::new();
    let mut first = 0;
    let mut width = 0;
    for (i, strip) in strips.iter().enumerate() {
        if width + strip.width > MAX_SHEET_WIDTH {
            sheets.push(&strips[first..i]);
            first = i;
            width = 0;
        }
        width += strip.width;
    }
    sheets.push(&strips[first..]);
    sheets
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
                    params[8] = reference_x(*r);
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
                    Some((index, classify(r, data)?))
                })
                .collect();
            ctx.Unmap(&self.readback, 0);
            Ok(Poll::Ready(result))
        }
    }
}

fn classify(rect: RECT, flags: &[u8]) -> Option<Columns> {
    let runs = strips(rect, flags)?;
    let mut exceptions: Vec<RECT> = Vec::new();
    for &r in &runs {
        if flags[(r.left - rect.left) as usize] & 4 != 0 {
            if let Some(last) = exceptions.last_mut()
                && last.right == r.left
            {
                last.right = r.right;
            } else {
                exceptions.push(r);
            }
        }
    }
    Some(Columns {
        rect,
        runs,
        exceptions,
    })
}

fn strips(rect: RECT, flags: &[u8]) -> Option<Vec<RECT>> {
    let mut start = rect.left;
    let mut result = Vec::new();
    for (i, &flag) in flags.iter().enumerate() {
        if flag & 2 != 0 {
            if diagnostics() {
                println!(
                    "{{\"event\":\"gpu-chart-backdrop-limit\",\"reason\":\"translucent-prefix\",\"columns\":{},\"height\":{}}}",
                    flags.iter().filter(|&&f| f & 2 != 0).count(),
                    rect.bottom - rect.top
                );
            }
            return None;
        }
        if i != 0 && flag & 1 != 0 {
            let x = rect.left + i as i32;
            result.push(RECT {
                left: start,
                right: x,
                ..rect
            });
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
    use super::{MAX_SHEET_WIDTH, MAX_STRIPS, Strip, classify, pack, reference_x, sheets, strips};
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
            4,
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
    fn strips_cover_the_same_columns_and_reject_translucent_backdrops() {
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
    }

    #[test]
    fn narrow_glyph_columns_pack_together_and_split_at_the_texture_limit() {
        let rect = RECT {
            left: 0,
            right: 120,
            top: 5,
            bottom: 25,
        };
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(5.)),
            size(ScaledPixels(300.), ScaledPixels(20.)),
        );
        let runs = strips(rect, &[1; 120]).unwrap();
        let columns = pack(&runs, bounds, 4).unwrap();
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].clip, rect);
        assert!(columns[0].stride > 1);
        assert!(columns[0].fits(bounds));
        assert!(columns[0].stride.is_power_of_two());
        assert_eq!(columns[0].phase(-0.125) - columns[0].phase(0.), 1.);
        let many: Vec<_> = (0..=MAX_STRIPS as i32)
            .map(|i| RECT {
                left: i * 8,
                right: (i + 1) * 8,
                ..rect
            })
            .collect();
        assert!(pack(&many, bounds, 4).is_none());
        assert_eq!(sheets(&columns).len(), 1);
        let large = [
            Strip {
                width: MAX_SHEET_WIDTH - 1,
                ..columns[0]
            },
            Strip {
                width: 1,
                ..columns[0]
            },
            columns[0],
        ];
        let packed = sheets(&large);
        assert_eq!(packed.iter().map(|s| s.len()).collect::<Vec<_>>(), [2, 1]);
    }

    #[test]
    fn phase_lanes_keep_the_same_anchor_across_a_wide_backdrop() {
        let rect = RECT {
            left: 3000,
            right: 3600,
            top: 25,
            bottom: 55,
        };
        let bounds = Bounds::new(
            point(ScaledPixels(2990.375), ScaledPixels(25.)),
            size(ScaledPixels(1400.), ScaledPixels(80.)),
        );
        let runs = strips(rect, &[1; 600]).unwrap();
        let tiles = pack(&runs, bounds, 4).unwrap();
        assert!(tiles.len() > 1);
        let anchor = bounds.left().0.round();
        let mut next = rect.left;
        for tile in tiles {
            assert_eq!(tile.clip.left, next);
            assert_eq!(tile.width, (tile.clip.right - next) as u32 * tile.stride);
            assert!(tile.width <= MAX_SHEET_WIDTH && tile.fits(bounds));
            assert_eq!(tile.phase(anchor).fract(), 0.);
            let mut moved = bounds;
            moved.origin.x.0 -= bounds.size.width.0 / 60.;
            assert!(tile.fits(moved));
            next = tile.clip.right;
        }
        assert_eq!(next, rect.right);
    }

    #[test]
    fn sparse_columns_preserve_every_grid_edge_and_share_one_phase() {
        let rect = RECT {
            left: 19,
            right: 129,
            top: 3,
            bottom: 20,
        };
        let bounds = Bounds::new(
            point(ScaledPixels(17.375), ScaledPixels(2.)),
            size(ScaledPixels(112.), ScaledPixels(40.)),
        );
        // Two neighboring grid columns have different colors, followed by a
        // return to the reference and an isolated line near the far edge.
        let mut flags = [0; 110];
        for (x, f) in [(3, 5), (4, 5), (5, 1), (95, 5), (96, 1)] {
            flags[x] = f;
        }
        let profile = classify(rect, &flags).unwrap();
        let plan = profile.sparse(bounds, 4).unwrap();
        assert_eq!(plan.clips(), 2);
        assert!(plan.bytes(20) <= plan.bytes(80));
        let base = plan.strips[0];
        let lanes = plan.corrections;
        assert_eq!(base.clip, rect);
        assert_eq!(base.column, reference_x(rect));
        assert_eq!(
            lanes
                .iter()
                .map(|s| (s.clip.left, s.clip.right))
                .collect::<Vec<_>>(),
            [(22, 24), (114, 115)]
        );
        assert_eq!(
            lanes[0].phase(bounds.left().0.round()),
            lanes[1].phase(bounds.left().0.round())
        );
        assert!(
            lanes
                .iter()
                .all(|s| s.fits(bounds) && s.column == s.clip.left)
        );
        flags[1] = 6;
        assert!(classify(rect, &flags).is_none());
    }
}
