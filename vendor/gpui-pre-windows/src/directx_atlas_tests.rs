//! Argus patch: tests of the atlas changes (ink rows, chart page sizes and
//! spare pages), inside the atlas tests to share their helpers.
use super::*;

#[test]
fn chart_ink_extent_preserves_even_a_single_faint_texel() {
    let mut pixels = [0; 3 * 6 * 4];
    pixels[0] = 255; // Transparent RGB makes no contribution.
    assert_eq!(ink_rows(&pixels, 3, 6), 0..0);
    pixels[(2 * 3 + 2) * 4 + 3] = 1;
    assert_eq!(ink_rows(&pixels, 3, 6), 2..3);
    pixels[5 * 3 * 4 + 3] = 255;
    assert_eq!(ink_rows(&pixels, 3, 6), 2..6);
}

#[test]
fn wide_chart_pages_fit_the_ink_rows_and_reject_oversized_tiles() {
    let Some(atlas) = create_atlas() else { return };
    let size = |w, h| Size {
        width: DevicePixels(w),
        height: DevicePixels(h),
    };
    let key = make_image_key(17);
    let tile = insert_tile(&atlas, key.clone(), size(4080, 114));
    {
        let lock = atlas.0.lock();
        let page = lock.backend.texture(tile.texture_id).unwrap();
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { page.texture.GetDesc(&mut desc) };
        assert_eq!((desc.Width, desc.Height), (4096, 128));
    }
    atlas.remove(&key);
    let mut lock = atlas.0.lock();
    assert!(lock.backend.texture(tile.texture_id).is_none());
    assert!(
        lock.backend
            .allocate(size(2_776_631, 114), AtlasTextureKind::Polychrome)
            .is_none()
    );
    assert_eq!(
        lock.backend
            .polychrome_textures
            .textures
            .iter()
            .flatten()
            .count(),
        0
    );
}

#[test]
fn chart_revisions_reuse_the_device_page_and_replace_its_ink() {
    let Some(atlas) = create_atlas() else { return };
    atlas.0.lock().backend.chart_rows = Some(Default::default());
    atlas.finish_page_updates(true);
    let insert = |key: AtlasKey, w, h, ink: usize| {
        atlas
            .get_or_insert_with(key, &mut || {
                let mut bytes = vec![0; (w * h * 4) as usize];
                bytes[ink * w as usize * 4 + 3] = 255;
                Ok(Some((
                    Size {
                        width: DevicePixels(w),
                        height: DevicePixels(h),
                    },
                    Cow::Owned(bytes),
                )))
            })
            .unwrap()
            .unwrap()
    };
    let old = make_image_key(20);
    let a = insert(old.clone(), 1560, 105, 6);
    let view = atlas.get_texture_view(a.texture_id).unwrap();
    assert_eq!(atlas.chart_rows(&a), 6..7);
    atlas.remove(&old);
    assert!(atlas.get_texture_view(a.texture_id).is_none());
    let next = make_image_key(21);
    let b = insert(next.clone(), 1580, 117, 113);
    assert_eq!(atlas.get_texture_view(b.texture_id).unwrap(), view);
    assert_eq!(atlas.chart_rows(&b), 113..114);
    atlas.remove(&next);
    // A short upload must not expose data from either earlier revision.
    assert!(
        atlas
            .get_or_insert_with(make_image_key(22), &mut || Ok(Some((
                Size {
                    width: DevicePixels(1580),
                    height: DevicePixels(117)
                },
                Cow::Borrowed(&[0; 4])
            ))))
            .is_err()
    );
    assert!(atlas.get_texture_view(b.texture_id).is_none());
    assert!(
        atlas
            .0
            .lock()
            .backend
            .chart_rows
            .as_ref()
            .unwrap()
            .is_empty()
    );
    atlas.finish_page_updates(true);
    assert!(atlas.0.lock().backend.spares.is_empty());
}

#[test]
fn spare_pages_are_bounded_and_released_when_the_window_is_inactive() {
    let Some(atlas) = create_atlas() else { return };
    atlas.finish_page_updates(true);
    let mut lock = atlas.0.lock();
    let backend = &mut lock.backend;
    let allocate = |backend: &mut DirectXAtlasTextures, w, h| {
        backend
            .allocate(
                Size {
                    width: DevicePixels(w),
                    height: DevicePixels(h),
                },
                AtlasTextureKind::Polychrome,
            )
            .unwrap()
    };
    let tiles = (0..MAX_SPARE_PAGES + 2)
        .map(|_| allocate(backend, 1088, 64))
        .collect::<Vec<_>>();
    for t in tiles {
        backend.remove(t);
    }
    assert_eq!(backend.spares.len(), MAX_SPARE_PAGES);
    backend.finish_page_updates(true);
    let a = allocate(backend, 4096, 640);
    let b = allocate(backend, 4096, 640);
    backend.remove(a);
    backend.remove(b);
    assert_eq!(backend.spares.len(), 1);
    assert!(
        backend
            .spares
            .iter()
            .map(DirectXAtlasTexture::bytes)
            .sum::<u64>()
            <= MAX_SPARE_BYTES
    );
    backend.finish_page_updates(false);
    assert!(backend.spares.is_empty());
    let inactive = allocate(backend, 4096, 640);
    backend.remove(inactive);
    assert!(backend.spares.is_empty());
    assert_eq!(
        backend.polychrome_textures.free_list.len(),
        backend.polychrome_textures.textures.len()
    );
}
