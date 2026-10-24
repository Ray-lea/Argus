use super::{split_batches, suffix_damage};
use gpui::{Bounds, ContentMask, PrimitiveBatch, Quad, ScaledPixels, Scene, point, size};

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
    let rect = suffix_damage(&scene, &[PrimitiveBatch::Quads(0..1)], 100, 80).unwrap();
    assert_eq!(
        (rect.left, rect.top, rect.right, rect.bottom),
        (9, 14, 91, 76)
    );
}
