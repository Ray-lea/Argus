use super::{
    independent_sprite_regions, merge_damage, split_batches, subtract_rect, suffix_damage,
};
use gpui::{Bounds, ContentMask, PrimitiveBatch, Quad, ScaledPixels, Scene, point, size};
use windows::Win32::Foundation::RECT;

#[test]
fn replay_snaps_chart_frames_and_rules_after_the_composed_transform() {
    use gpui::{Axis, Edges, Point, VisualTransform};

    let bounds = |x, y, w, h| {
        Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(w), ScaledPixels(h)),
        )
    };
    let mask = ContentMask {
        bounds: bounds(0., 0., 300., 300.),
    };
    let mut source = Scene::default();
    source.insert_primitive(Quad {
        bounds: bounds(10., 10., 120., 70.),
        content_mask: ContentMask {
            bounds: bounds(10., 10., 120., 3.),
        },
        border_widths: Edges::all(ScaledPixels(1.)),
        border_color: gpui::white(),
        ..Default::default()
    });
    source.insert_rule(
        Axis::Horizontal,
        Quad {
            bounds: bounds(11., 30., 118., 1.),
            content_mask: mask,
            background: gpui::white().into(),
            ..Default::default()
        },
    );
    source.insert_rule(
        Axis::Vertical,
        Quad {
            bounds: bounds(50., 11., 1., 68.),
            content_mask: mask,
            background: gpui::white().into(),
            ..Default::default()
        },
    );

    for (dx, dy, sx, sy, lengths, expected_width) in [
        (0.5, -0.49, 1., 1., 1., 1.),
        (0.24, 0.7, 0.985, 0.985, 0.985, 1.),
        (-1.2, 2.4, 1.9, 0.6, 1., 1.),
        (0.2, 0.4, 2., 2., 2., 2.),
    ] {
        let mut frame = Scene::default();
        frame.push_transform(VisualTransform::moved(
            point(ScaledPixels(dx), ScaledPixels(dy)),
            0.5,
        ));
        frame.push_transform(VisualTransform {
            scale_x: sx,
            scale_y: sy,
            lengths,
            ..VisualTransform::moved(Point::default(), 1.)
        });
        frame.replay(0..source.len(), &source);
        frame.pop_transform();
        frame.pop_transform();
        assert_eq!(frame.quads.len(), 3);
        let border = &frame.quads[0];
        assert_eq!(border.border_widths.top.0, expected_width);
        assert_eq!(border.border_color.a, 0.5);
        assert_eq!(frame.quads[1].bounds.size.height.0, expected_width);
        assert_eq!(frame.quads[2].bounds.size.width.0, expected_width);
        for rect in frame
            .quads
            .iter()
            .map(|q| q.bounds)
            .chain([border.content_mask.bounds])
        {
            for value in [rect.left(), rect.top(), rect.right(), rect.bottom()] {
                assert_eq!(value.0.fract(), 0.);
            }
        }
        // A second replay uses the original rule geometry, not the previously scaled pixels.
        let mut settled = Scene::default();
        settled.replay(2..2 + source.len(), &frame);
        for (a, b) in settled.quads.iter().zip(&source.quads) {
            assert_eq!(a.bounds, b.bounds);
        }
    }
}

#[test]
fn animated_boundary_splits_a_shared_batch_without_reordering() {
    let mut scene = Scene::default();
    scene.quads = [0, 1, 2, 2]
        .map(|order| Quad {
            order,
            ..Default::default()
        })
        .to_vec();
    let batches = split_batches(&scene, Some(2));
    assert_eq!(batches.len(), 2);
    assert!(matches!(&batches[0], PrimitiveBatch::Quads(range) if *range == (0..2)));
    assert!(matches!(&batches[1], PrimitiveBatch::Quads(range) if *range == (2..4)));
}

#[test]
fn damage_restores_the_mask_when_a_bar_shrinks_to_zero() {
    let mut scene = Scene::default();
    scene.quads.push(Quad {
        bounds: Bounds::new(
            point(ScaledPixels(20.), ScaledPixels(70.)),
            size(ScaledPixels(4.), ScaledPixels(0.)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(10.), ScaledPixels(15.)),
                size(ScaledPixels(80.), ScaledPixels(60.)),
            ),
        },
        ..Default::default()
    });
    let rects = suffix_damage(&scene, &[PrimitiveBatch::Quads(0..1)], 100, 80).unwrap();
    assert_eq!(rects.len(), 1);
    let rect = rects[0];
    assert_eq!(
        (rect.left, rect.top, rect.right, rect.bottom),
        (9, 14, 91, 76)
    );
}

#[test]
fn distant_damage_keeps_static_gap_out_of_present_and_copy() {
    let mut regions = Vec::new();
    merge_damage(
        &mut regions,
        RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        },
    );
    merge_damage(
        &mut regions,
        RECT {
            left: 80,
            top: 80,
            right: 100,
            bottom: 100,
        },
    );
    assert_eq!(regions.len(), 2);
    assert_eq!(
        regions
            .iter()
            .map(|r| (r.right - r.left) * (r.bottom - r.top))
            .sum::<i32>(),
        800
    );
}

#[test]
fn bridging_damage_merges_all_overlaps_before_blending() {
    let mut regions = vec![
        RECT {
            left: 0,
            top: 0,
            right: 20,
            bottom: 20,
        },
        RECT {
            left: 40,
            top: 0,
            right: 60,
            bottom: 20,
        },
    ];
    merge_damage(
        &mut regions,
        RECT {
            left: 15,
            top: 5,
            right: 45,
            bottom: 15,
        },
    );
    assert_eq!(
        regions,
        vec![RECT {
            left: 0,
            top: 0,
            right: 60,
            bottom: 20
        }]
    );
}

#[test]
fn unsupported_suffix_requires_full_frame() {
    assert!(suffix_damage(&Scene::default(), &[PrimitiveBatch::Shadows(0..1)], 100, 80).is_none());
}

fn sprite(x: f32, y: f32, width: f32, height: f32) -> gpui::PolychromeSprite {
    let bounds = Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(width), ScaledPixels(height)),
    );
    gpui::PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false.into(),
        opacity: 1.,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        tile: gpui::AtlasTile {
            texture_id: gpui::AtlasTextureId {
                index: 0,
                kind: gpui::AtlasTextureKind::Polychrome,
            },
            tile_id: gpui::TileId(0),
            padding: 0,
            bounds: Default::default(),
        },
    }
}

#[test]
fn independent_sprites_restore_exact_regions_without_covering_gaps() {
    let mut scene = Scene::default();
    scene.polychrome_sprites = vec![sprite(0., 0., 20., 20.), sprite(90., 70., 30., 30.)];
    let batches = scene.batches().collect::<Vec<_>>();
    assert_eq!(
        independent_sprite_regions(&scene, &batches, 100, 80).map(|plan| plan.dirty),
        Some(vec![
            RECT {
                left: 0,
                top: 0,
                right: 20,
                bottom: 20
            },
            RECT {
                left: 90,
                top: 70,
                right: 100,
                bottom: 80
            },
        ])
    );
}

#[test]
fn overlapping_or_unsupported_content_keeps_the_original_blend_path() {
    let mut scene = Scene::default();
    scene.polychrome_sprites = vec![sprite(0., 0., 20., 20.), sprite(19., 0., 20., 20.)];
    assert!(
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).is_none()
    );
    assert!(
        independent_sprite_regions(&scene, &[PrimitiveBatch::Shadows(0..1)], 100, 80).is_none()
    );
    scene.polychrome_sprites.truncate(1);
    scene.polychrome_sprites[0].bounds.size.width = ScaledPixels(0.);
    assert!(
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).is_none()
    );
}

#[test]
fn later_bars_reuse_the_background_restored_by_an_enclosing_image() {
    let mut scene = Scene::default();
    scene.polychrome_sprites.push(sprite(0., 0., 50., 50.));
    scene.quads.push(Quad {
        order: 1,
        bounds: Bounds::new(
            point(ScaledPixels(5.), ScaledPixels(15.)),
            size(ScaledPixels(5.), ScaledPixels(0.)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(5.), ScaledPixels(5.)),
                size(ScaledPixels(10.), ScaledPixels(10.)),
            ),
        },
        ..Default::default()
    });
    let plan =
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).unwrap();
    assert!(plan.restore.is_empty());
    assert_eq!(plan.dirty.len(), 1);
    scene.quads[0].order = 0;
    assert!(
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).is_none()
    );
}

#[test]
fn a_later_bar_copies_only_background_outside_the_chart() {
    let mut scene = Scene::default();
    scene.polychrome_sprites.push(sprite(0., 20., 100., 50.));
    scene.quads.push(Quad {
        order: 1,
        bounds: Bounds::new(
            point(ScaledPixels(10.), ScaledPixels(5.)),
            size(ScaledPixels(4.), ScaledPixels(15.)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(10.), ScaledPixels(0.)),
                size(ScaledPixels(4.), ScaledPixels(20.)),
            ),
        },
        ..Default::default()
    });
    let plan =
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).unwrap();
    assert_eq!(
        plan.restore,
        vec![RECT {
            left: 9,
            top: 0,
            right: 15,
            bottom: 20
        }]
    );
    scene.quads[0].order = 0;
    assert!(
        independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 100, 80).is_none()
    );
}

#[test]
fn rectangle_subtraction_preserves_only_the_uncovered_pixels() {
    let rect = RECT {
        left: 0,
        top: 0,
        right: 11,
        bottom: 9,
    };
    for cover in [
        RECT {
            left: 3,
            top: 2,
            right: 7,
            bottom: 6,
        },
        RECT {
            left: -3,
            top: -2,
            right: 7,
            bottom: 6,
        },
        RECT {
            left: 11,
            top: 0,
            right: 15,
            bottom: 9,
        },
        RECT {
            left: -1,
            top: -1,
            right: 12,
            bottom: 10,
        },
    ] {
        let pieces = subtract_rect(rect, cover).collect::<Vec<_>>();
        for y in -2..12 {
            for x in -3..16 {
                let contains = |r: RECT| r.left <= x && x < r.right && r.top <= y && y < r.bottom;
                let copies = pieces.iter().filter(|r| contains(**r)).count();
                assert_eq!(copies, usize::from(contains(rect) && !contains(cover)));
            }
        }
    }
}

#[test]
fn fractional_image_edges_can_restore_a_larger_rounded_mask() {
    for scale in [1., 1.2, 1.5] {
        let mut image = sprite(10.25 * scale, 20.25 * scale, 100. * scale, 40. * scale);
        image.content_mask.bounds = Bounds::new(
            point(
                ScaledPixels((10.25 * scale).floor()),
                ScaledPixels((20.25 * scale).floor()),
            ),
            size(
                ScaledPixels((110.25 * scale).ceil() - (10.25 * scale).floor()),
                ScaledPixels((60.25 * scale).ceil() - (20.25 * scale).floor()),
            ),
        );
        let mut scene = Scene::default();
        scene.polychrome_sprites.push(image);
        let plan =
            independent_sprite_regions(&scene, &scene.batches().collect::<Vec<_>>(), 500, 300)
                .unwrap();
        assert_eq!(plan.dirty.len(), 1);
        assert!(plan.restore.is_empty());
    }
}
