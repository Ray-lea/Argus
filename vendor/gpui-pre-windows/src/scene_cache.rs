//! Compare the actual static draws across paints before repairing their cached pixels.
use super::*;

/// Atlas tiles uploaded since the last frame was drawn, by texture and tile
/// id. A removed tile's id goes to the next image of its size, so an
/// unchanged primitive can still draw other pixels when its tile is here.
pub(super) type FreshTiles = std::collections::HashSet<(AtlasTextureId, u32)>;

/// The bounds of a glyph drawn through `transformation`.
fn glyph_bounds(
    bounds: Bounds<ScaledPixels>,
    transformation: TransformationMatrix,
) -> Bounds<ScaledPixels> {
    if transformation == TransformationMatrix::unit() {
        return bounds;
    }
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for (x, y) in [
        (bounds.left(), bounds.top()),
        (bounds.right(), bounds.top()),
        (bounds.left(), bounds.bottom()),
        (bounds.right(), bounds.bottom()),
    ] {
        let p = transformation.apply(point(gpui::px(x.0), gpui::px(y.0)));
        for (i, value) in [f32::from(p.x), f32::from(p.y)].into_iter().enumerate() {
            low[i] = low[i].min(value);
            high[i] = high[i].max(value);
        }
    }
    Bounds::from_corners(
        point(ScaledPixels(low[0]), ScaledPixels(low[1])),
        point(ScaledPixels(high[0]), ScaledPixels(high[1])),
    )
}

/// A kind of primitive whose instances are compared across frames.
trait Ink: Copy + PartialEq {
    /// Where it can draw: its bounds within its mask.
    fn ink(&self) -> Bounds<ScaledPixels>;
    /// The atlas tile it shows.
    fn tile(&self) -> Option<&AtlasTile> {
        None
    }
}

impl Ink for Quad {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl Ink for Underline {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl Ink for MonochromeSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        glyph_bounds(self.bounds, self.transformation).intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
}

impl Ink for SubpixelSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        glyph_bounds(self.bounds, self.transformation).intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
}

impl Ink for PolychromeSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
}

/// The device pixels `ink` can touch, `None` when it has no area.
fn ink_rect(ink: Bounds<ScaledPixels>, width: u32, height: u32) -> Option<RECT> {
    let rect = RECT {
        left: ink.left().0.floor().clamp(0., width as f32) as i32,
        top: ink.top().0.floor().clamp(0., height as f32) as i32,
        right: ink.right().0.ceil().clamp(0., width as f32) as i32,
        bottom: ink.bottom().0.ceil().clamp(0., height as f32) as i32,
    };
    (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
}

fn is_fresh<T: Ink>(item: &T, fresh: &FreshTiles) -> bool {
    item.tile()
        .is_some_and(|tile| fresh.contains(&(tile.texture_id, tile.tile_id.0)))
}

/// Puts into `local`, emptied first, the primitives of `batches` whose ink
/// can reach one of `rects`, in their order. Drawn under a scissor inside the
/// rects they give the same pixels as the complete scene: the rest cannot
/// touch them. False for paths, shadows and surfaces.
pub(super) fn local_scene(
    local: &mut Scene,
    scene: &Scene,
    batches: &[PrimitiveBatch],
    rects: &[RECT],
) -> bool {
    local.quads.clear();
    local.underlines.clear();
    local.monochrome_sprites.clear();
    local.subpixel_sprites.clear();
    local.polychrome_sprites.clear();
    // Ink lies in both its bounds and its mask; two pixels of margin cover
    // the rounding out to whole pixels and the pixel to spare.
    let grown = rects
        .iter()
        .map(|r| {
            [
                r.left as f32 - 2.,
                r.top as f32 - 2.,
                r.right as f32 + 2.,
                r.bottom as f32 + 2.,
            ]
        })
        .collect::<Vec<_>>();
    let hull = grown.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |a, r| {
            [
                a[0].min(r[0]),
                a[1].min(r[1]),
                a[2].max(r[2]),
                a[3].max(r[3]),
            ]
        },
    );
    let meets = |b: &Bounds<ScaledPixels>, r: &[f32; 4]| {
        b.origin.x.0 < r[2]
            && r[0] < b.origin.x.0 + b.size.width.0
            && b.origin.y.0 < r[3]
            && r[1] < b.origin.y.0 + b.size.height.0
    };
    let reaches = |b: &Bounds<ScaledPixels>| {
        meets(b, &hull) && (grown.len() == 1 || grown.iter().any(|r| meets(b, r)))
    };
    for batch in batches {
        match batch {
            PrimitiveBatch::Quads(r) => local.quads.extend(
                scene.quads[r.clone()]
                    .iter()
                    .filter(|q| reaches(&q.content_mask.bounds) && reaches(&q.bounds)),
            ),
            PrimitiveBatch::Underlines(r) => local.underlines.extend(
                scene.underlines[r.clone()]
                    .iter()
                    .filter(|u| reaches(&u.content_mask.bounds) && reaches(&u.bounds)),
            ),
            PrimitiveBatch::MonochromeSprites { range, .. } => local.monochrome_sprites.extend(
                scene.monochrome_sprites[range.clone()].iter().filter(|s| {
                    reaches(&s.content_mask.bounds)
                        && reaches(&glyph_bounds(s.bounds, s.transformation))
                }),
            ),
            PrimitiveBatch::SubpixelSprites { range, .. } => {
                local
                    .subpixel_sprites
                    .extend(scene.subpixel_sprites[range.clone()].iter().filter(|s| {
                        reaches(&s.content_mask.bounds)
                            && reaches(&glyph_bounds(s.bounds, s.transformation))
                    }))
            }
            PrimitiveBatch::PolychromeSprites { range, .. } => local.polychrome_sprites.extend(
                scene.polychrome_sprites[range.clone()]
                    .iter()
                    .filter(|s| reaches(&s.content_mask.bounds) && reaches(&s.bounds)),
            ),
            _ => return false,
        }
    }
    true
}

/// The ink of some batches of a scene, per kind of primitive in draw order.
/// Equal kinds draw the same pixels: their draw orders decide how the kinds
/// interleave.
#[derive(Default)]
pub(super) struct Snapshot {
    quads: Vec<Quad>,
    underlines: Vec<Underline>,
    mono: Vec<MonochromeSprite>,
    subpixel: Vec<SubpixelSprite>,
    images: Vec<PolychromeSprite>,
}

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

    pub(super) fn new(scene: &Scene, batches: &[PrimitiveBatch]) -> Option<Self> {
        let mut snapshot = Self::default();
        snapshot.fill(scene, batches).then_some(snapshot)
    }

    /// Takes the ink of `batches` into the buffers of `self`. False for
    /// paths, shadows and external surfaces, which keep the complete paint
    /// path, and for scenes too large to compare.
    pub(super) fn fill(&mut self, scene: &Scene, batches: &[PrimitiveBatch]) -> bool {
        self.quads.clear();
        self.underlines.clear();
        self.mono.clear();
        self.subpixel.clear();
        self.images.clear();
        for batch in batches {
            match batch {
                PrimitiveBatch::Quads(r) => self.quads.extend_from_slice(&scene.quads[r.clone()]),
                PrimitiveBatch::Underlines(r) => self
                    .underlines
                    .extend_from_slice(&scene.underlines[r.clone()]),
                PrimitiveBatch::MonochromeSprites { range, .. } => self
                    .mono
                    .extend_from_slice(&scene.monochrome_sprites[range.clone()]),
                PrimitiveBatch::SubpixelSprites { range, .. } => self
                    .subpixel
                    .extend_from_slice(&scene.subpixel_sprites[range.clone()]),
                PrimitiveBatch::PolychromeSprites { range, .. } => self
                    .images
                    .extend_from_slice(&scene.polychrome_sprites[range.clone()]),
                _ => return false,
            }
        }
        self.quads.len()
            + self.underlines.len()
            + self.mono.len()
            + self.subpixel.len()
            + self.images.len()
            <= 20_000
    }

    pub(super) fn matches(&self, other: &Self, fresh: &FreshTiles) -> bool {
        self.quads == other.quads
            && self.underlines == other.underlines
            && self.mono == other.mono
            && self.subpixel == other.subpixel
            && self.images == other.images
            && (fresh.is_empty()
                || !(self.mono.iter().any(|s| is_fresh(s, fresh))
                    || self.subpixel.iter().any(|s| is_fresh(s, fresh))
                    || self.images.iter().any(|s| is_fresh(s, fresh))))
    }

    pub(super) fn repair(
        &self,
        previous: &Self,
        width: u32,
        height: u32,
        fresh: &FreshTiles,
    ) -> Option<Repair> {
        let mut damage: Option<RECT> = None;
        diff(
            &self.quads,
            &previous.quads,
            width,
            height,
            fresh,
            &mut damage,
        );
        diff(
            &self.underlines,
            &previous.underlines,
            width,
            height,
            fresh,
            &mut damage,
        );
        diff(
            &self.mono,
            &previous.mono,
            width,
            height,
            fresh,
            &mut damage,
        );
        diff(
            &self.subpixel,
            &previous.subpixel,
            width,
            height,
            fresh,
            &mut damage,
        );
        diff(
            &self.images,
            &previous.images,
            width,
            height,
            fresh,
            &mut damage,
        );
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

/// Adds to `damage` where `now` and `old` differ: both places of a changed
/// primitive, and the primitives only one of them has.
fn diff<T: Ink>(
    now: &[T],
    old: &[T],
    width: u32,
    height: u32,
    fresh: &FreshTiles,
    damage: &mut Option<RECT>,
) {
    let mut include = |item: &T| {
        if let Some(rect) = ink_rect(item.ink(), width, height) {
            *damage = Some(damage.map_or(rect, |r| RECT {
                left: r.left.min(rect.left),
                top: r.top.min(rect.top),
                right: r.right.max(rect.right),
                bottom: r.bottom.max(rect.bottom),
            }));
        }
    };
    let common = now.len().min(old.len());
    let check = !fresh.is_empty();
    for (a, b) in now[..common].iter().zip(&old[..common]) {
        if a != b || (check && is_fresh(a, fresh)) {
            include(a);
            include(b);
        }
    }
    now[common..].iter().for_each(&mut include);
    old[common..].iter().for_each(&mut include);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quad(x: f32, y: f32) -> Quad {
        let bounds = Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(20.), ScaledPixels(10.)),
        );
        Quad {
            bounds,
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(200.), ScaledPixels(100.)),
                ),
            },
            ..Default::default()
        }
    }

    fn quads(quads: Vec<Quad>) -> Snapshot {
        Snapshot {
            quads,
            ..Default::default()
        }
    }

    #[::core::prelude::v1::test]
    fn changed_and_removed_ink_restore_both_old_and_new_positions() {
        let before = quads(vec![quad(10., 10.), quad(60., 60.)]);
        let after = quads(vec![quad(20., 15.)]);
        let none = FreshTiles::default();
        let Some(Repair::Region(r)) = after.repair(&before, 200, 100, &none) else {
            panic!("local repair expected");
        };
        assert_eq!((r.left, r.top, r.right, r.bottom), (10, 10, 80, 70));
        assert!(matches!(
            after.repair(&after, 200, 100, &none),
            Some(Repair::Unchanged)
        ));
    }

    #[::core::prelude::v1::test]
    fn a_tile_uploaded_again_repaints_the_unchanged_primitive_that_shows_it() {
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: 2,
                kind: AtlasTextureKind::Polychrome,
            },
            tile_id: TileId(7),
            padding: 0,
            bounds: Bounds::new(
                point(DevicePixels(0), DevicePixels(0)),
                size(DevicePixels(16), DevicePixels(16)),
            ),
        };
        let image = PolychromeSprite {
            order: 1,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds: Bounds::new(
                point(ScaledPixels(100.), ScaledPixels(5.)),
                size(ScaledPixels(16.), ScaledPixels(16.)),
            ),
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(200.), ScaledPixels(100.)),
                ),
            },
            corner_radii: Corners::default(),
            tile,
        };
        let scene = Snapshot {
            quads: vec![quad(10., 10.)],
            images: vec![image],
            ..Default::default()
        };
        let fresh = FreshTiles::from([(tile.texture_id, tile.tile_id.0)]);
        let Some(Repair::Region(r)) = scene.repair(&scene, 200, 100, &fresh) else {
            panic!("the reused tile must be repainted");
        };
        assert_eq!((r.left, r.top, r.right, r.bottom), (100, 5, 116, 21));
        assert!(!scene.matches(&scene, &fresh));
        assert!(scene.matches(&scene, &FreshTiles::default()));
    }

    #[::core::prelude::v1::test]
    fn a_local_scene_keeps_what_reaches_the_rects_in_order() {
        let mut scene = Scene::default();
        for (order, x) in [(1, 10.), (2, 120.), (3, 30.)] {
            let mut q = quad(x, 10.);
            q.order = order;
            scene.quads.push(q);
        }
        let rect = RECT {
            left: 0,
            top: 0,
            right: 60,
            bottom: 40,
        };
        let mut local = Scene::default();
        assert!(local_scene(
            &mut local,
            &scene,
            &[PrimitiveBatch::Quads(0..3)],
            &[rect]
        ));
        let orders = local.quads.iter().map(|q| q.order).collect::<Vec<_>>();
        assert_eq!(orders, [1, 3]);
        // Two pixels of margin: the first quad covers x 10..30 and still
        // counts for a rect from x 31, not for one from x 32. A second rect
        // far away does not change that.
        let mut reach = |left| {
            let rect = RECT {
                left,
                top: 0,
                right: 40,
                bottom: 40,
            };
            let far = RECT {
                left: 150,
                top: 60,
                right: 160,
                bottom: 70,
            };
            assert!(local_scene(
                &mut local,
                &scene,
                &[PrimitiveBatch::Quads(0..1)],
                &[rect, far]
            ));
            local.quads.len()
        };
        assert_eq!((reach(31), reach(32)), (1, 0));
        assert!(!local_scene(
            &mut Scene::default(),
            &scene,
            &[PrimitiveBatch::Shadows(0..0)],
            &[rect]
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
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, true, None)?;
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
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, true, None)?;
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
            renderer.render(&scene, WindowBackgroundAppearance::Transparent, false, None)?;
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
                renderer.render(&scene, WindowBackgroundAppearance::Transparent, true, None)?;
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

    /// A hidden window with a renderer of its own; the window is destroyed
    /// on drop. Nothing is shown, activated or captured.
    struct HiddenRenderer {
        renderer: DirectXRenderer,
        window: HWND,
    }

    impl Drop for HiddenRenderer {
        fn drop(&mut self) {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.window);
            }
        }
    }

    fn hidden_renderer(
        devices: &DirectXDevices,
        width: i32,
        height: i32,
    ) -> Result<HiddenRenderer> {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };
        use windows::core::w;
        let window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Argus renderer test"),
                WS_POPUP,
                0,
                0,
                width,
                height,
                None,
                None,
                None,
                None,
            )?
        };
        let mut renderer = DirectXRenderer::new(window, devices, false)?;
        renderer.resize(size(DevicePixels(width), DevicePixels(height)))?;
        Ok(HiddenRenderer { renderer, window })
    }

    fn test_image(renderer: &DirectXRenderer, id: usize) -> Result<AtlasTile> {
        let pixels: Vec<u8> = (0..16 * 16)
            .flat_map(|p| {
                let alpha = ((p % 16) * 12 + 40) as u8;
                [alpha / 2, alpha / 3, alpha, alpha]
            })
            .collect();
        renderer
            .atlas
            .get_or_insert_with(
                AtlasKey::Image(RenderImageParams {
                    image_id: ImageId(id),
                    frame_index: 0,
                }),
                &mut || {
                    Ok(Some((
                        size(DevicePixels(16), DevicePixels(16)),
                        std::borrow::Cow::Borrowed(&pixels),
                    )))
                },
            )?
            .context("test image tile")
    }

    /// Stands in for the swap chain: `BUFFER_COUNT` targets used in turn, as
    /// a flip-model chain hands back the buffer it presented longest ago.
    struct Flips {
        targets: Vec<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)>,
        next: usize,
    }

    impl Flips {
        fn new(renderer: &mut DirectXRenderer, width: u32, height: u32) -> Result<Self> {
            let device = renderer.devices.as_ref().context("devices")?.device.clone();
            let mut targets = Vec::new();
            for _ in 0..BUFFER_COUNT {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: RENDER_TARGET_FORMAT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..Default::default()
                };
                let (mut texture, mut view) = (None, None);
                unsafe {
                    device.CreateTexture2D(&desc, None, Some(&mut texture))?;
                    let texture = texture.context("flip target")?;
                    device.CreateRenderTargetView(&texture, None, Some(&mut view))?;
                    targets.push((texture, view));
                }
            }
            let mut flips = Self { targets, next: 0 };
            flips.flip(renderer);
            renderer.resources.as_mut().context("resources")?.forget();
            Ok(flips)
        }

        fn flip(&mut self, renderer: &mut DirectXRenderer) {
            let r = renderer.resources.as_mut().expect("resources");
            r.frames.flipped();
            r.frames.presented();
            let (texture, view) = self.targets[self.next].clone();
            r.render_target = Some(texture);
            r.render_target_view = view;
            self.next = (self.next + 1) % self.targets.len();
        }
    }

    #[::core::prelude::v1::test]
    fn partial_frames_bring_every_flip_buffer_up_to_date() -> Result<()> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        let _com = Com;
        let devices = DirectXDevices::new()?;
        let mut tested = hidden_renderer(&devices, 200, 100)?;
        let mut complete = hidden_renderer(&devices, 200, 100)?;
        let tiles = (
            test_image(&tested.renderer, 938_131)?,
            test_image(&complete.renderer, 938_131)?,
        );
        let mut flips = Flips::new(&mut tested.renderer, 200, 100)?;
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            Bounds::new(
                point(ScaledPixels(x), ScaledPixels(y)),
                size(ScaledPixels(w), ScaledPixels(h)),
            )
        };
        let window = ContentMask {
            bounds: rect(0., 0., 200., 100.),
        };
        // The pointer crosses rows one at a time and jumps; the bar of the
        // chart variant changes on some frames only; frames 9 and 10 change
        // the whole background, frame 11 a row again.
        let hovered = [0, 1, 2, 2, 3, 0, 4, 1, 1, 3, 3, 2, 4, 4, 0, 1];
        let bars = [
            10., 10., 20., 20., 20., 5., 5., 5., 30., 30., 30., 30., 12., 12., 12., 40.,
        ];
        let image = |tile: AtlasTile, order, bounds: Bounds<ScaledPixels>| PolychromeSprite {
            order,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            tile,
        };
        let scene = |step: usize, chart: bool, tile: AtlasTile| {
            let mut scene = Scene::default();
            scene.paint_revision = 7_000 + 100 * u64::from(chart) + step as u64;
            let background = if (9..11).contains(&step) {
                gpui::rgba(0x40302080 + step as u32)
            } else {
                gpui::rgba(0x203040c0)
            };
            scene.quads.push(Quad {
                order: 0,
                bounds: window.bounds,
                content_mask: window,
                background: background.into(),
                ..Default::default()
            });
            for row in 0..5 {
                scene.quads.push(Quad {
                    order: 1,
                    bounds: rect(10., 10. + row as f32 * 15., 120., 12.),
                    content_mask: window,
                    background: if row == hovered[step] {
                        gpui::rgba(0xd0e0f070).into()
                    } else {
                        gpui::rgba(0x10203030).into()
                    },
                    corner_radii: Corners::all(ScaledPixels(3.)),
                    ..Default::default()
                });
            }
            scene
                .polychrome_sprites
                .push(image(tile, 2, rect(12.5, 40., 10., 10.)));
            if chart {
                scene.animated_order = Some(3);
                let height = bars[step];
                scene.quads.push(Quad {
                    order: 3,
                    bounds: rect(150., 90. - height, 30., height),
                    content_mask: ContentMask {
                        bounds: rect(150., 20., 30., 70.),
                    },
                    background: gpui::rgba(0x20bbee80).into(),
                    ..Default::default()
                });
                // A still image in the suffix lets it compose the prefix.
                scene
                    .polychrome_sprites
                    .push(image(tile, 4, rect(100., 60., 16., 16.)));
            }
            scene
        };
        // A part of the rows clear of the suffix's image, as a chart backdrop
        // patch shows it: the suffix waits outside, and the frame is not
        // presented by a flip. The rows it changed reach under the image.
        let rows = RECT {
            left: 8,
            top: 8,
            right: 98,
            bottom: 84,
        };
        for (chart, patched) in [(false, false), (true, false), (true, true)] {
            for step in 0..hovered.len() {
                let frame = scene(step, chart, tiles.0);
                let only = (patched && step % 2 == 0).then_some(rows);
                tested.renderer.render(
                    &frame,
                    WindowBackgroundAppearance::Transparent,
                    true,
                    only,
                )?;
                let partial = tested.renderer.read_render_target()?;
                complete.renderer.resources.as_mut().unwrap().forget();
                let reference = complete.renderer.render_to_image(
                    &scene(step, chart, tiles.1),
                    WindowBackgroundAppearance::Transparent,
                )?;
                // An image that composes the prefix in its shader rounds its
                // blend apart from the output merger: by a level at most.
                let composed =
                    |x: u32, y: u32| chart && (100..116).contains(&x) && (60..76).contains(&y);
                let shown = |x: u32, y: u32| {
                    only.is_none_or(|r| {
                        (r.left as u32..r.right as u32).contains(&x)
                            && (r.top as u32..r.bottom as u32).contains(&y)
                    })
                };
                let mut worst = (0u8, 0u32, 0u32);
                for (x, y, pixel) in partial.enumerate_pixels() {
                    if !shown(x, y) {
                        continue;
                    }
                    let delta = pixel
                        .0
                        .iter()
                        .zip(reference.get_pixel(x, y).0)
                        .map(|(a, b)| a.abs_diff(b))
                        .max()
                        .unwrap_or(0);
                    let delta = if composed(x, y) && delta <= 2 {
                        0
                    } else {
                        delta
                    };
                    if delta > worst.0 {
                        worst = (delta, x, y);
                    }
                }
                assert!(
                    worst.0 == 0,
                    "frame {step} (chart: {chart}, patched: {}) differs from a complete paint by {} at {:?}",
                    only.is_some(),
                    worst.0,
                    (worst.1, worst.2)
                );
                if only.is_none() {
                    flips.flip(&mut tested.renderer);
                }
            }
        }
        Ok(())
    }
}
