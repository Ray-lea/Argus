//! Compare the actual static draws across paints before repairing their cached pixels.
use super::*;

#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Quad(Quad),
    Underline(Underline),
    Mono(MonochromeSprite),
    Subpixel(SubpixelSprite),
    Image(PolychromeSprite),
}

impl Ink {
    fn bounds(self) -> Bounds<ScaledPixels> {
        let (bounds, mask) = match self {
            Self::Quad(q) => (q.bounds, q.content_mask),
            Self::Underline(u) => (u.bounds, u.content_mask),
            Self::Mono(s) => (
                chart_composition::transformed_glyph_bounds(&s),
                s.content_mask,
            ),
            Self::Subpixel(s) => {
                let mono = MonochromeSprite {
                    order: s.order,
                    is_glyph: 1,
                    bounds: s.bounds,
                    content_mask: s.content_mask,
                    color: s.color,
                    tile: s.tile,
                    transformation: s.transformation,
                };
                (
                    chart_composition::transformed_glyph_bounds(&mono),
                    s.content_mask,
                )
            }
            Self::Image(s) => (s.bounds, s.content_mask),
        };
        bounds.intersect(&mask.bounds)
    }
}

pub(super) struct Snapshot(Vec<Ink>);

#[derive(Clone, Copy)]
pub(super) enum Repair {
    Unchanged,
    Region(RECT),
}

impl Snapshot {
    pub(super) fn stationary(scene: &Scene) -> Option<Self> {
        let (_, batches) = chart_composition::separate_foreground(scene, scene.batches().collect());
        Self::new(scene, &batches)
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    pub(super) fn new(scene: &Scene, batches: &[PrimitiveBatch]) -> Option<Self> {
        let mut ink = Vec::new();
        for batch in batches {
            match batch {
                PrimitiveBatch::Quads(r) => {
                    ink.extend(scene.quads[r.clone()].iter().copied().map(Ink::Quad))
                }
                PrimitiveBatch::Underlines(r) => ink.extend(
                    scene.underlines[r.clone()]
                        .iter()
                        .copied()
                        .map(Ink::Underline),
                ),
                PrimitiveBatch::MonochromeSprites { range, .. } => ink.extend(
                    scene.monochrome_sprites[range.clone()]
                        .iter()
                        .copied()
                        .map(Ink::Mono),
                ),
                PrimitiveBatch::SubpixelSprites { range, .. } => ink.extend(
                    scene.subpixel_sprites[range.clone()]
                        .iter()
                        .copied()
                        .map(Ink::Subpixel),
                ),
                PrimitiveBatch::PolychromeSprites { range, .. } => ink.extend(
                    scene.polychrome_sprites[range.clone()]
                        .iter()
                        .copied()
                        .map(Ink::Image),
                ),
                // Paths, shadows and external surfaces retain the complete paint path.
                _ => return None,
            }
            if ink.len() > 20_000 {
                return None;
            }
        }
        Some(Self(ink))
    }

    pub(super) fn repair(&self, previous: &Self, width: u32, height: u32) -> Option<Repair> {
        let mut damage: Option<RECT> = None;
        for i in 0..self.0.len().max(previous.0.len()) {
            let (now, old) = (self.0.get(i), previous.0.get(i));
            if now == old {
                continue;
            }
            for ink in [now, old].into_iter().flatten() {
                let b = ink.bounds();
                let rect = RECT {
                    left: b.left().0.floor().clamp(0., width as f32) as i32,
                    top: b.top().0.floor().clamp(0., height as f32) as i32,
                    right: b.right().0.ceil().clamp(0., width as f32) as i32,
                    bottom: b.bottom().0.ceil().clamp(0., height as f32) as i32,
                };
                if rect.left >= rect.right || rect.top >= rect.bottom {
                    continue;
                }
                damage = Some(damage.map_or(rect, |r| RECT {
                    left: r.left.min(rect.left),
                    top: r.top.min(rect.top),
                    right: r.right.max(rect.right),
                    bottom: r.bottom.max(rect.bottom),
                }));
            }
        }
        match damage {
            None => Some(Repair::Unchanged),
            Some(r)
                if (r.right - r.left) as u64 * (r.bottom - r.top) as u64 * 4
                    < u64::from(width) * u64::from(height) * 3 =>
            {
                Some(Repair::Region(r))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quad(x: f32, y: f32) -> Ink {
        let bounds = Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(20.), ScaledPixels(10.)),
        );
        Ink::Quad(Quad {
            bounds,
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(200.), ScaledPixels(100.)),
                ),
            },
            ..Default::default()
        })
    }

    #[::core::prelude::v1::test]
    fn changed_and_removed_ink_restore_both_old_and_new_positions() {
        let before = Snapshot(vec![quad(10., 10.), quad(60., 60.)]);
        let after = Snapshot(vec![quad(20., 15.)]);
        let Some(Repair::Region(r)) = after.repair(&before, 200, 100) else {
            panic!("local repair expected");
        };
        assert_eq!((r.left, r.top, r.right, r.bottom), (10, 10, 80, 70));
        assert!(matches!(
            after.repair(&after, 200, 100),
            Some(Repair::Unchanged)
        ));
    }

    #[::core::prelude::v1::test]
    fn animated_partial_prefix_matches_a_complete_gpu_paint() -> Result<()> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };
        use windows::core::w;
        struct OwnedWindow(HWND);
        impl Drop for OwnedWindow {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                    CoUninitialize();
                }
            }
        }
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        // The window remains hidden for the entire test. Pixel readback uses
        // only its renderer's private target; no desktop capture or activation.
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Argus renderer test"),
                WS_POPUP,
                0,
                0,
                200,
                100,
                None,
                None,
                None,
                None,
            )?
        });
        let devices = DirectXDevices::new()?;
        let mut renderer = DirectXRenderer::new(window.0, &devices, false)?;
        renderer.resize(size(DevicePixels(200), DevicePixels(100)))?;
        for step in 0..12 {
            let mut scene = Scene::default();
            scene.paint_revision = step + 1;
            scene.animated_order = Some(3);
            let rect = |x, y, w, h| {
                Bounds::new(
                    point(ScaledPixels(x), ScaledPixels(y)),
                    size(ScaledPixels(w), ScaledPixels(h)),
                )
            };
            let mask = ContentMask {
                bounds: rect(0., 0., 200., 100.),
            };
            scene.quads.push(Quad {
                order: 0,
                bounds: mask.bounds,
                content_mask: mask,
                background: gpui::rgba(0x203040c0).into(),
                ..Default::default()
            });
            if step != 10 {
                scene.quads.push(Quad {
                    order: 1,
                    bounds: rect(10. + step as f32, 15., 60., 40.),
                    content_mask: mask,
                    background: gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(gpui::white().opacity(step as f32 / 12.), 0.),
                        gpui::linear_color_stop(gpui::white().opacity(0.), 1.),
                    ),
                    corner_radii: Corners::all(ScaledPixels(6.)),
                    ..Default::default()
                });
            }
            // A changing suffix can shrink or disappear; its old region must be restored.
            if step != 9 {
                let bounds = rect(120., 40. + step as f32, 40., 30. - step as f32);
                scene.quads.push(Quad {
                    order: 3,
                    bounds,
                    content_mask: ContentMask {
                        bounds: rect(120., 40., 40., 40.),
                    },
                    background: gpui::rgba(0x20bbee80).into(),
                    ..Default::default()
                });
            } else {
                scene.quads.push(Quad {
                    order: 3,
                    bounds: rect(155., 70., 5., 5.),
                    content_mask: ContentMask {
                        bounds: rect(155., 70., 5., 5.),
                    },
                    background: gpui::white().into(),
                    ..Default::default()
                });
            }
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, true)?;
            let repaired = renderer.read_render_target()?;
            renderer.resources.as_mut().unwrap().prefix_key = None;
            let reference =
                renderer.render_to_image(&scene, WindowBackgroundAppearance::Transparent)?;
            assert_eq!(repaired, reference, "partial paint diverged at step {step}");
        }
        let pixels: Vec<u8> = (0..80 * 80)
            .flat_map(|p| {
                let alpha = ((p / 80) * 2 + 20) as u8;
                [alpha / 3, alpha, alpha / 2, alpha]
            })
            .collect();
        let tile = renderer
            .atlas
            .get_or_insert_with(
                AtlasKey::Image(RenderImageParams {
                    image_id: ImageId(938_121),
                    frame_index: 0,
                }),
                &mut || {
                    Ok(Some((
                        size(DevicePixels(80), DevicePixels(80)),
                        std::borrow::Cow::Borrowed(&pixels),
                    )))
                },
            )?
            .context("rescale test tile")?;
        for (index, height) in [40., 55.25, 20.5, 8.75, 40.].into_iter().enumerate() {
            let mut scene = Scene::default();
            scene.paint_revision = 40;
            // Full interaction paints have no bounds-update marker yet.
            scene.animated_order = None;
            let mask = ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(200.), ScaledPixels(100.)),
                ),
            };
            scene.quads.push(Quad {
                bounds: mask.bounds,
                content_mask: mask,
                background: gpui::rgb(0x202124).into(),
                ..Default::default()
            });
            scene.quads.push(Quad {
                order: 1,
                bounds: mask.bounds,
                content_mask: mask,
                background: gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(gpui::rgb(0x304050), 0.),
                    gpui::linear_color_stop(gpui::rgb(0x202124), 1.),
                ),
                ..Default::default()
            });
            scene.polychrome_sprites.push(PolychromeSprite {
                order: 3,
                pad: PolychromeSprite::IN_SCENE_AREA_FILTER,
                grayscale: false.into(),
                opacity: 1.,
                bounds: Bounds::new(
                    point(ScaledPixels(120.25), ScaledPixels(90. - height)),
                    size(ScaledPixels(40.), ScaledPixels(height)),
                ),
                content_mask: ContentMask {
                    bounds: Bounds::new(
                        point(ScaledPixels(120.), ScaledPixels(35.)),
                        size(ScaledPixels(41.), ScaledPixels(55.)),
                    ),
                },
                corner_radii: Corners::default(),
                tile,
            });
            scene.quads.push(Quad {
                order: 2,
                bounds: Bounds::new(
                    point(ScaledPixels(20.), ScaledPixels(42.)),
                    size(ScaledPixels(25.), ScaledPixels(15.)),
                ),
                content_mask: mask,
                background: gpui::rgba(0x30708060).into(),
                ..Default::default()
            });
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, true)?;
            let repaired = renderer.read_render_target()?;
            renderer.resources.as_mut().unwrap().prefix_key = None;
            let reference =
                renderer.render_to_image(&scene, WindowBackgroundAppearance::Transparent)?;
            let delta = repaired
                .as_raw()
                .iter()
                .zip(reference.as_raw())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(delta <= 2, "rescale pixels diverged at {index}: {delta}");
            scene.animated_order = Some(3);
            scene.paint_revision = 60 + index as u64;
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, false)?;
            renderer.prepare_dynamic_test(&scene);
            if index == 0 {
                let mut installed = false;
                for _ in 0..100 {
                    if renderer
                        .install_chart_tree(&scene, WindowBackgroundAppearance::Transparent)?
                    {
                        installed = true;
                        break;
                    }
                    unsafe {
                        devices.device_context.Flush();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(installed, "dynamic tree did not install");
            } else {
                assert!(
                    renderer.update_chart_tree(&scene, WindowBackgroundAppearance::Transparent)?
                );
            }
            let (rect, texture) = renderer
                .dynamic_test_texture()
                .context("dynamic chart texture")?;
            let previous = renderer
                .resources
                .as_mut()
                .unwrap()
                .render_target
                .replace(texture);
            let local = renderer.read_render_target()?;
            renderer.resources.as_mut().unwrap().render_target = previous;
            let local = image::imageops::crop_imm(
                &local,
                0,
                0,
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            )
            .to_image();
            let expected = image::imageops::crop_imm(
                &reference,
                rect.left as u32,
                rect.top as u32,
                local.width(),
                local.height(),
            )
            .to_image();
            let delta = local
                .as_raw()
                .iter()
                .zip(expected.as_raw())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(
                delta <= 2,
                "dynamic chart pixels diverged at {index}: {delta}"
            );
            assert!(
                local.pixels().all(|pixel| pixel.0[3] == 255),
                "dynamic chart lost its opaque backdrop"
            );
            let prior = scene.quads[0].background;
            scene.quads[0].background = gpui::rgb(0x102030).into();
            scene.paint_revision += 10;
            assert!(
                !renderer.update_chart_tree(&scene, WindowBackgroundAppearance::Transparent)?,
                "changed static pixels reused an obsolete tree"
            );
            scene.quads[0].background = prior;
            if index == 4 {
                scene.quads[2].background = gpui::rgba(0x50803080).into();
                let patch = renderer
                    .plan_chart_backdrop(&scene, WindowBackgroundAppearance::Transparent)
                    .context("local backdrop patch")?;
                renderer.render(&scene, WindowBackgroundAppearance::Transparent, true)?;
                assert!(renderer.patch_chart_backdrop(
                    &scene,
                    WindowBackgroundAppearance::Transparent,
                    patch
                )?);
                let patched = renderer.read_render_target()?;
                renderer.resources.as_mut().unwrap().prefix_key = None;
                let full =
                    renderer.render_to_image(&scene, WindowBackgroundAppearance::Transparent)?;
                let delta = patched
                    .as_raw()
                    .iter()
                    .zip(full.as_raw())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                assert!(delta <= 2, "backdrop patch diverged: {delta}");
                scene.quads[2].bounds.origin.x = ScaledPixels(125.);
                scene.paint_revision += 1;
                assert!(
                    renderer
                        .plan_chart_backdrop(&scene, WindowBackgroundAppearance::Transparent)
                        .is_none(),
                    "overlapping static ink bypassed chart rebaking"
                );
            }
        }
        Ok(())
    }
}
