use super::{DirectWriteTextSystem, rasterizing_mode};
use anyhow::Result;
use gpui::{
    Font, FontRun, PlatformTextSystem, RenderGlyphParams, TextRun, TextSystem, WindowTextSystem,
    point, px,
};
use std::sync::Arc;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_HIT_TEST_METRICS, DWRITE_RENDERING_MODE1_NATURAL,
    DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC, DWRITE_RENDERING_MODE1_OUTLINE, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, IDWriteTextLayout,
};

#[test]
fn grayscale_diagonals_keep_the_lcd_footprint_and_intermediate_coverage() -> Result<()> {
    let system = DirectWriteTextSystem::new()?;
    let font_id = system.font_id(&gpui::font("Segoe UI"))?;
    let glyph_id = system
        .glyph_for_char(font_id, 'A')
        .expect("Segoe UI capital A");
    let [wr, wg, wb] = system.components.grayscale_weights;
    let mut levels = [false; 256];
    for scale in [1.0, 1.25, 1.5] {
        for x in 0..gpui::SUBPIXEL_VARIANTS_X as u8 {
            let gray = RenderGlyphParams {
                font_id,
                glyph_id,
                font_size: px(18.),
                subpixel_variant: point(x, 0),
                scale_factor: scale,
                is_emoji: false,
                subpixel_rendering: false,
                dilation: 0,
            };
            let lcd = RenderGlyphParams {
                subpixel_rendering: true,
                ..gray.clone()
            };
            let bounds = system.glyph_raster_bounds(&gray)?;
            assert_eq!(bounds, system.glyph_raster_bounds(&lcd)?);
            let (size, alpha) = system.rasterize_glyph(&gray, bounds)?;
            let (lcd_size, rgba) = system.rasterize_glyph(&lcd, bounds)?;
            assert_eq!(size, lcd_size);
            assert_eq!(alpha.len(), (size.width.0 * size.height.0) as usize);
            assert_eq!(rgba.len(), 4 * alpha.len());
            for (&a, pixel) in alpha.iter().zip(rgba.chunks_exact(4)) {
                let expected = (wr * u16::from(pixel[0])
                    + wg * u16::from(pixel[1])
                    + wb * u16::from(pixel[2])
                    + 128)
                    >> 8;
                assert_eq!(u16::from(a), expected);
                levels[a as usize] = true;
            }
        }
    }
    assert!(levels.into_iter().filter(|used| *used).count() > 32);
    Ok(())
}

#[test]
fn fallback_glyphs_keep_the_requested_fonts_ascent_and_descent() -> Result<()> {
    let system = DirectWriteTextSystem::new()?;
    let font_id = system.font_id(&gpui::font("Segoe UI"))?;
    let metrics = system.font_metrics(font_id);
    let size = px(14.);
    let expected_ascent = size * (metrics.ascent / metrics.units_per_em as f32);
    let expected_descent = size * (-metrics.descent / metrics.units_per_em as f32);
    // Latin alone, Chinese alone (laid out in a fallback font where the
    // system has one) and both.
    for text in [
        "Argus",
        "\u{8fdb}\u{7a0b}",
        "Argus \u{5f00}\u{53d1}\u{7248}",
    ] {
        let layout = system.layout_line(
            text,
            size,
            &[FontRun {
                len: text.len(),
                font_id,
            }],
        );
        assert!(
            (layout.ascent - expected_ascent).abs() < px(0.001),
            "{text}"
        );
        assert!(
            (layout.descent - expected_descent).abs() < px(0.001),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn rendering_follows_the_recommended_smoothing() {
    assert_eq!(
        rasterizing_mode(DWRITE_RENDERING_MODE1_NATURAL),
        DWRITE_RENDERING_MODE1_NATURAL
    );
    assert_eq!(
        rasterizing_mode(DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC),
        DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC
    );
    assert_eq!(
        rasterizing_mode(DWRITE_RENDERING_MODE1_OUTLINE),
        DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC
    );
}

/// Microsoft YaHei UI at 13 px asks for horizontal smoothing only. Drawn
/// that way, the stem of 忄 in 性 reaches the row of the bottom stroke of
/// 生, as in Windows and Chromium; with vertical smoothing on the same
/// hints it stopped a row short. Skipped where the font is not installed.
#[test]
fn hinted_chinese_strokes_keep_their_length() -> Result<()> {
    let system = DirectWriteTextSystem::new()?;
    let Ok(font_id) = system.font_id(&gpui::font("Microsoft YaHei UI")) else {
        return Ok(());
    };
    let Some(glyph_id) = system.glyph_for_char(font_id, '\u{6027}') else {
        return Ok(());
    };
    let params = RenderGlyphParams {
        font_id,
        glyph_id,
        font_size: px(13.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.0,
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    };
    let bounds = system.glyph_raster_bounds(&params)?;
    let (size, alpha) = system.rasterize_glyph(&params, bounds)?;
    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    let inked = |column: usize, row: usize| alpha[row * width + column] >= 128;
    let last_row = (0..height)
        .rev()
        .find(|&row| (0..width).any(|column| inked(column, row)))
        .expect("inked glyph");
    // The stem is the tallest column in the left third.
    let stem = (0..width / 3)
        .max_by_key(|&column| (0..height).filter(|&row| inked(column, row)).count())
        .expect("left third");
    assert!(
        inked(stem, last_row),
        "the stem ends above the bottom stroke"
    );
    Ok(())
}

#[test]
fn right_to_left_runs_are_laid_out_in_visual_order() -> Result<()> {
    let platform_text_system = Arc::new(DirectWriteTextSystem::new()?);
    let font = Font {
        family: "Segoe UI".into(),
        ..Default::default()
    };
    platform_text_system.font_id(&font)?;
    let text_system =
        WindowTextSystem::new(Arc::new(TextSystem::new(platform_text_system.clone())));
    // The byte index of each glyph from left to right: a right-to-left
    // run reads from its right edge, and numbers inside it left to right.
    let font_id = platform_text_system.font_id(&font)?;
    for (text, visual_order) in [
        ("مستكشف", vec![10, 8, 6, 4, 2, 0]),
        (
            "abc مستكشف def",
            vec![0, 1, 2, 3, 14, 12, 10, 8, 6, 4, 16, 17, 18, 19],
        ),
        ("مستكشف.exe", vec![10, 8, 6, 4, 2, 0, 12, 13, 14, 15]),
        ("مستكشف 2", vec![13, 12, 10, 8, 6, 4, 2, 0]),
        ("אבג 123 דה", vec![13, 11, 10, 7, 8, 9, 6, 4, 2, 0]),
        ("a\tאב\tb", vec![0, 1, 4, 2, 6, 7]),
        // Marks stay after their bases, at the same index.
        ("مُسْتَكْشِف", vec![20, 16, 16, 12, 12, 8, 8, 4, 4, 0, 0]),
    ] {
        for font_size in [px(12.0), px(14.0), px(16.0)] {
            let runs = [TextRun {
                len: text.len(),
                font: font.clone(),
                ..Default::default()
            }];
            let shaped = text_system.shape_line(text.into(), font_size, &runs, None);
            let glyphs = shaped
                .runs
                .iter()
                .flat_map(|run| run.glyphs.iter().map(|glyph| (run.font_id, glyph)))
                .collect::<Vec<_>>();
            assert_eq!(
                glyphs
                    .iter()
                    .map(|(_, glyph)| glyph.index)
                    .collect::<Vec<_>>(),
                visual_order,
                "{text:?}, {font_size:?}"
            );
            let layout = native_layout(&platform_text_system, font_id, font_size, text)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            unsafe { layout.GetMetrics(&mut metrics)? };
            assert!(
                (shaped.width - px(metrics.widthIncludingTrailingWhitespace)).abs() < px(0.001),
                "{text:?}, {font_size:?}"
            );
            // Each cluster's first glyph sits where DirectWrite draws it in
            // the cluster DirectWrite hit-tests.
            let mut previous_index = None;
            for (glyph_font_id, glyph) in &glyphs {
                if previous_index.replace(glyph.index) == Some(glyph.index) {
                    continue;
                }
                let position = text[..glyph.index].encode_utf16().count() as u32;
                let mut cluster = DWRITE_HIT_TEST_METRICS::default();
                let (mut x, mut y) = (0.0, 0.0);
                unsafe {
                    layout.HitTestTextPosition(position, false, &mut x, &mut y, &mut cluster)?
                };
                let mut expected = cluster.left;
                if cluster.bidiLevel % 2 == 1 {
                    let design_advance = platform_text_system
                        .advance(*glyph_font_id, glyph.id)?
                        .width
                        * font_size.as_f32()
                        / platform_text_system
                            .font_metrics(*glyph_font_id)
                            .units_per_em as f32;
                    expected += cluster.width - design_advance;
                }
                assert!(
                    (glyph.position.x - px(expected)).abs() < px(0.001),
                    "{text:?}, {font_size:?}: the cluster at {} starts at {:?}, not {expected}",
                    glyph.index,
                    glyph.position.x,
                );
            }
        }
    }
    Ok(())
}

fn native_layout(
    system: &DirectWriteTextSystem,
    font_id: gpui::FontId,
    font_size: gpui::Pixels,
    text: &str,
) -> Result<IDWriteTextLayout> {
    let state = system.state.read();
    let font = &state.fonts[font_id.0];
    let text = text.encode_utf16().collect::<Vec<_>>();
    unsafe {
        let format = system.components.factory.CreateTextFormat(
            &font.font_family_h,
            &font.font_collection,
            font.font_face.GetWeight(),
            font.font_face.GetStyle(),
            DWRITE_FONT_STRETCH_NORMAL,
            font_size.as_f32(),
            &system.components.locale,
        )?;
        let layout = system.components.factory.CreateTextLayout(
            &text,
            &format,
            f32::INFINITY,
            f32::INFINITY,
        )?;
        // The features GPUI's layouts ask for.
        layout.SetTypography(
            &font.features,
            DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: text.len() as u32,
            },
        )?;
        Ok(layout)
    }
}

#[test]
fn right_to_left_glyphs_sit_where_direct_write_draws_them() -> Result<()> {
    use super::{design_advances, place_glyph};
    use std::mem::ManuallyDrop;
    use windows::Win32::{
        Foundation::RECT,
        Graphics::DirectWrite::{
            DWRITE_GLYPH_OFFSET, DWRITE_GLYPH_RUN, DWRITE_GRID_FIT_MODE_DISABLED,
            DWRITE_MEASURING_MODE_NATURAL, DWRITE_RENDERING_MODE1_ALIASED,
            DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE, DWRITE_TEXTURE_ALIASED_1x1,
        },
    };
    use windows::core::{BOOL, Interface};

    let system = DirectWriteTextSystem::new()?;
    let font_id = system.font_id(&gpui::font("Segoe UI"))?;
    let state = system.state.read();
    let font_face = &state.fonts[font_id.0].font_face;
    let characters = ['م', '\u{64f}', 'س', '\u{652}', 'ت'].map(u32::from);
    let mut glyph_ids = [0u16; 5];
    unsafe {
        font_face.GetGlyphIndices(
            characters.as_ptr(),
            characters.len() as u32,
            glyph_ids.as_mut_ptr(),
        )?;
    }
    assert!(glyph_ids.iter().all(|&id| id != 0));
    // At 32 px, Segoe UI's 2048 design units per em make every design
    // advance a whole number of 64ths of a pixel; with quarter pixels for
    // the rest, every sum is exact.
    const EM_SIZE: f32 = 32.0;
    let advances = [8.25f32, 0.0, 11.75, 0.0, 5.5];
    let offsets = [
        (0.0, 0.0),
        (-1.25, 4.5),
        (0.0, 0.0),
        (2.5, -2.25),
        (0.75, 0.0),
    ]
    .map(|(advance_offset, ascender_offset)| DWRITE_GLYPH_OFFSET {
        advanceOffset: advance_offset,
        ascenderOffset: ascender_offset,
    });
    let (origin_x, origin_y) = (64.0, 32.0);
    let coverage = |glyph_ids: &[u16],
                    advances: &[f32],
                    offsets: &[DWRITE_GLYPH_OFFSET],
                    bidi_level: u32,
                    origin_x: f32|
     -> Result<(RECT, Vec<u8>)> {
        let glyph_run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(font_face.cast()?)),
            fontEmSize: EM_SIZE,
            glyphCount: glyph_ids.len() as u32,
            glyphIndices: glyph_ids.as_ptr(),
            glyphAdvances: advances.as_ptr(),
            glyphOffsets: offsets.as_ptr(),
            isSideways: BOOL(0),
            bidiLevel: bidi_level,
        };
        let result = unsafe {
            system
                .components
                .factory
                .CreateGlyphRunAnalysis(
                    &glyph_run,
                    None,
                    DWRITE_RENDERING_MODE1_ALIASED,
                    DWRITE_MEASURING_MODE_NATURAL,
                    DWRITE_GRID_FIT_MODE_DISABLED,
                    DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                    origin_x,
                    origin_y,
                )
                .and_then(|analysis| {
                    let bounds = analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1)?;
                    let mut alpha = vec![
                        0u8;
                        ((bounds.right - bounds.left) * (bounds.bottom - bounds.top))
                            as usize
                    ];
                    analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &bounds, &mut alpha)?;
                    Ok((bounds, alpha))
                })
        };
        drop(ManuallyDrop::into_inner(glyph_run.fontFace));
        Ok(result?)
    };

    // Advances unlike the glyphs' own (as kerning makes them), with
    // offsets either way: DirectWrite sets a right-to-left glyph by its
    // design advance and moves the pen by its advance in the run.
    let expected = coverage(&glyph_ids, &advances, &offsets, 1, origin_x)?;
    let design_advances = design_advances(font_face, EM_SIZE, BOOL(0), &glyph_ids)?;
    assert_ne!(design_advances.as_slice(), advances.as_slice());
    // Draw each glyph where `place_glyph` puts it, left to right, the way
    // GPUI draws a line's glyphs one by one.
    let mut pen = origin_x;
    let placed = advances
        .iter()
        .zip(&offsets)
        .zip(&design_advances)
        .map(
            |((&advance, offset), &design_advance)| DWRITE_GLYPH_OFFSET {
                advanceOffset: place_glyph(
                    &mut pen,
                    advance,
                    offset.advanceOffset,
                    Some(design_advance),
                ),
                ascenderOffset: offset.ascenderOffset,
            },
        )
        .collect::<Vec<_>>();
    let actual = coverage(&glyph_ids, &[0.0; 5], &placed, 0, 0.0)?;
    assert_eq!(pen, origin_x - advances.iter().sum::<f32>());
    assert_eq!(
        (actual.0.left, actual.0.top, actual.0.right, actual.0.bottom),
        (
            expected.0.left,
            expected.0.top,
            expected.0.right,
            expected.0.bottom
        )
    );
    assert!(actual.1 == expected.1, "the glyphs are drawn elsewhere");
    Ok(())
}
