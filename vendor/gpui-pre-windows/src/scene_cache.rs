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
    fn mask(&self) -> Bounds<ScaledPixels>;
    fn order(&self) -> DrawOrder;
    /// The same primitive moved by `by`, its mask too when `with_mask`;
    /// `None` when moving its bounds does not move its ink by `by`.
    fn shifted(&self, by: Point<ScaledPixels>, with_mask: bool) -> Option<Self>;
    fn bounds(&self) -> Bounds<ScaledPixels>;
    /// The same primitive within another mask.
    fn with_mask(&self, mask: Bounds<ScaledPixels>) -> Self;
    /// A fill of one color, but for its corner radius and border width (the
    /// largest of each): `None` for anything else.
    fn plain_fill(&self) -> Option<(f32, f32)> {
        None
    }
}

macro_rules! movable {
    () => {
        movable!(|_: &Self| true);
    };
    // `$moves`: whether its pixels move with its bounds.
    ($moves:expr) => {
        fn mask(&self) -> Bounds<ScaledPixels> {
            self.content_mask.bounds
        }
        fn bounds(&self) -> Bounds<ScaledPixels> {
            self.bounds
        }
        fn with_mask(&self, mask: Bounds<ScaledPixels>) -> Self {
            let mut masked = *self;
            masked.content_mask.bounds = mask;
            masked
        }
        fn order(&self) -> DrawOrder {
            self.order
        }
        fn shifted(&self, by: Point<ScaledPixels>, with_mask: bool) -> Option<Self> {
            if !($moves)(self) {
                return None;
            }
            let mut moved = *self;
            moved.bounds.origin = moved.bounds.origin + by;
            if with_mask {
                moved.content_mask.bounds.origin = moved.content_mask.bounds.origin + by;
            }
            Some(moved)
        }
    };
}

macro_rules! movable_glyph {
    () => {
        fn mask(&self) -> Bounds<ScaledPixels> {
            self.content_mask.bounds
        }
        fn bounds(&self) -> Bounds<ScaledPixels> {
            self.bounds
        }
        fn with_mask(&self, mask: Bounds<ScaledPixels>) -> Self {
            let mut masked = *self;
            masked.content_mask.bounds = mask;
            masked
        }
        fn order(&self) -> DrawOrder {
            self.order
        }
        fn shifted(&self, by: Point<ScaledPixels>, with_mask: bool) -> Option<Self> {
            // A transformation turns or scales the move as well.
            if self.transformation != TransformationMatrix::unit() {
                return None;
            }
            let mut moved = *self;
            moved.bounds.origin = moved.bounds.origin + by;
            if with_mask {
                moved.content_mask.bounds.origin = moved.content_mask.bounds.origin + by;
            }
            Some(moved)
        }
    };
}

impl Ink for Quad {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
    // A gradient is dithered by screen position (`gradient_color`): moved,
    // it draws other pixels.
    movable!(|quad: &Self| quad.background.as_solid().is_some());
    fn plain_fill(&self) -> Option<(f32, f32)> {
        let w = self.border_widths;
        let r = self.corner_radii;
        self.background.as_solid().is_some().then(|| {
            let corner = r
                .top_left
                .0
                .max(r.top_right.0)
                .max(r.bottom_left.0)
                .max(r.bottom_right.0);
            (corner, w.top.0.max(w.right.0).max(w.bottom.0).max(w.left.0))
        })
    }
}

impl Ink for Underline {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
    movable!();
}

impl Ink for MonochromeSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        glyph_bounds(self.bounds, self.transformation).intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
    movable_glyph!();
}

impl Ink for SubpixelSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        glyph_bounds(self.bounds, self.transformation).intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
    movable_glyph!();
}

impl Ink for PolychromeSprite {
    fn ink(&self) -> Bounds<ScaledPixels> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
    fn tile(&self) -> Option<&AtlasTile> {
        Some(&self.tile)
    }
    movable!();
}

/// Where a shadow draws, as `shadow_vertex` places it: an inset shadow
/// within its element, a drop shadow with three blur radii of tail around it.
fn shadow_area(shadow: &Shadow) -> Bounds<ScaledPixels> {
    if shadow.inset != 0 {
        shadow.element_bounds
    } else {
        let margin = ScaledPixels(3. * shadow.blur_radius.0);
        Bounds {
            origin: point(shadow.bounds.origin.x - margin, shadow.bounds.origin.y - margin),
            size: size(
                shadow.bounds.size.width + margin * 2.,
                shadow.bounds.size.height + margin * 2.,
            ),
        }
    }
}

impl Ink for Shadow {
    fn ink(&self) -> Bounds<ScaledPixels> {
        shadow_area(self).intersect(&self.content_mask.bounds)
    }
    fn mask(&self) -> Bounds<ScaledPixels> {
        self.content_mask.bounds
    }
    fn order(&self) -> DrawOrder {
        self.order
    }
    fn bounds(&self) -> Bounds<ScaledPixels> {
        shadow_area(self)
    }
    fn with_mask(&self, mask: Bounds<ScaledPixels>) -> Self {
        let mut masked = *self;
        masked.content_mask.bounds = mask;
        masked
    }
    fn shifted(&self, by: Point<ScaledPixels>, with_mask: bool) -> Option<Self> {
        let mut moved = *self;
        moved.bounds.origin = moved.bounds.origin + by;
        moved.element_bounds.origin = moved.element_bounds.origin + by;
        if with_mask {
            moved.content_mask.bounds.origin = moved.content_mask.bounds.origin + by;
        }
        Some(moved)
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
/// touch them. False for paths and surfaces.
pub(super) fn local_scene(
    local: &mut Scene,
    scene: &Scene,
    batches: &[PrimitiveBatch],
    rects: &[RECT],
) -> bool {
    local.shadows.clear();
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
            PrimitiveBatch::Shadows(r) => local.shadows.extend(
                scene.shadows[r.clone()]
                    .iter()
                    .filter(|s| reaches(&s.content_mask.bounds) && reaches(&shadow_area(s))),
            ),
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
#[derive(Clone, Default)]
pub(super) struct Snapshot {
    shadows: Vec<Shadow>,
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
    /// paths and external surfaces, which keep the complete paint
    /// path, and for scenes too large to compare.
    pub(super) fn fill(&mut self, scene: &Scene, batches: &[PrimitiveBatch]) -> bool {
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.mono.clear();
        self.subpixel.clear();
        self.images.clear();
        for batch in batches {
            match batch {
                PrimitiveBatch::Shadows(r) => {
                    self.shadows.extend_from_slice(&scene.shadows[r.clone()])
                }
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
        self.shadows.len()
            + self.quads.len()
            + self.underlines.len()
            + self.mono.len()
            + self.subpixel.len()
            + self.images.len()
            <= 20_000
    }

    pub(super) fn matches(&self, other: &Self, fresh: &FreshTiles) -> bool {
        self.shadows == other.shadows
            && self.quads == other.quads
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
            &self.shadows,
            &previous.shadows,
            width,
            height,
            fresh,
            &mut damage,
        );
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

    /// What a frame must draw when the last frame's pixels inside `clip`,
    /// where a list scrolled by `by` whole device pixels, are copied there
    /// moved by `by`. Each primitive is matched with the last frame's, as it
    /// was or moved by `by`; whatever the copy cannot show is drawn. `None`
    /// when nothing of `clip` would be copied.
    pub(super) fn scrolled(
        &self,
        previous: &Self,
        clip: RECT,
        by: (i32, i32),
        width: u32,
        height: u32,
        fresh: &FreshTiles,
    ) -> Option<Scrolled> {
        let copied = RECT {
            left: clip.left.max(clip.left + by.0),
            top: clip.top.max(clip.top + by.1),
            right: clip.right.min(clip.right + by.0),
            bottom: clip.bottom.min(clip.bottom + by.1),
        };
        if copied.left >= copied.right || copied.top >= copied.bottom {
            return None;
        }
        let mut pass = ScrollPass {
            clip,
            copied,
            by,
            width,
            height,
            fresh,
            damage: Vec::new(),
            fixed: Vec::new(),
            lowest_moved: DrawOrder::MAX,
            moved: 0,
        };
        pass.match_kind(&self.shadows, &previous.shadows);
        pass.match_kind(&self.quads, &previous.quads);
        pass.match_kind(&self.underlines, &previous.underlines);
        pass.match_kind(&self.mono, &previous.mono);
        pass.match_kind(&self.subpixel, &previous.subpixel);
        pass.match_kind(&self.images, &previous.images);
        // A fixed draw under the copied pixels must show the same one move
        // away: one color, without its edges or corners in `clip`, below all
        // that moved. Anything else there is drawn again.
        let fixed = std::mem::take(&mut pass.fixed);
        for (order, ink, covers) in fixed {
            if !(covers && order < pass.lowest_moved) {
                pass.include_copied(ink);
                pass.include_copied(offset(ink, by));
            }
        }
        for piece in subtract_rect(clip, copied) {
            pass.include(piece);
        }
        Some(Scrolled {
            copied,
            damage: pass.damage,
            moved: pass.moved,
        })
    }
}

/// The plan for a frame that copies scrolled pixels (`Snapshot::scrolled`).
pub(super) struct Scrolled {
    /// Where the copied pixels land: the clip moved by the scroll, within it.
    pub copied: RECT,
    /// What must be drawn, in no particular order and possibly overlapping.
    pub damage: Vec<RECT>,
    /// The primitives the copy shows moved.
    pub moved: usize,
}

struct ScrollPass<'a> {
    clip: RECT,
    copied: RECT,
    by: (i32, i32),
    width: u32,
    height: u32,
    fresh: &'a FreshTiles,
    damage: Vec<RECT>,
    /// Draws that stayed in place over the copied pixels: their order,
    /// where, and whether they cover `clip` with one color.
    fixed: Vec<(DrawOrder, RECT, bool)>,
    lowest_moved: DrawOrder,
    moved: usize,
}

fn offset(rect: RECT, by: (i32, i32)) -> RECT {
    RECT {
        left: rect.left + by.0,
        top: rect.top + by.1,
        right: rect.right + by.0,
        bottom: rect.bottom + by.1,
    }
}

fn within(inner: RECT, outer: RECT) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

/// The bytes of a primitive. The scene's primitives are `repr(C)` without
/// padding: they are uploaded to the GPU as these bytes.
fn bytes<T: Copy>(item: &T) -> &[u8] {
    // SAFETY: `T` is one of the scene's `repr(C)` primitive types, which have
    // no padding bytes, so every byte of the value is initialized.
    unsafe { std::slice::from_raw_parts((item as *const T).cast::<u8>(), size_of::<T>()) }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0u64;
    for chunk in bytes.chunks(8) {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        hash = (hash.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    hash
}

impl ScrollPass<'_> {
    fn include(&mut self, rect: RECT) {
        let rect = RECT {
            left: rect.left.max(0),
            top: rect.top.max(0),
            right: rect.right.min(self.width as i32),
            bottom: rect.bottom.min(self.height as i32),
        };
        if rect.left < rect.right && rect.top < rect.bottom {
            self.damage.push(rect);
        }
    }

    /// Includes the parts of `rect` the copy does not replace.
    fn include_uncopied(&mut self, rect: RECT) {
        for piece in subtract_rect(rect, self.copied) {
            self.include(piece);
        }
    }

    /// Includes the part of `rect` the copy covers.
    fn include_copied(&mut self, rect: RECT) {
        let c = self.copied;
        self.include(RECT {
            left: rect.left.max(c.left),
            top: rect.top.max(c.top),
            right: rect.right.min(c.right),
            bottom: rect.bottom.min(c.bottom),
        });
    }

    fn rect<T: Ink>(&self, item: &T) -> Option<RECT> {
        ink_rect(item.ink(), self.width, self.height)
    }

    /// Whether a fill with `corner` radii and a `border` shows its one
    /// color over every pixel of the clip: the clip stays a pixel clear of
    /// its edges, its border and the squares around its corners.
    fn uniform_over_clip<T: Ink>(&self, item: &T, corner: f32, border: f32) -> bool {
        let ink = item.ink();
        let edge = border.ceil() as i32 + 1;
        let inner = RECT {
            left: ink.left().0.ceil() as i32 + edge,
            top: ink.top().0.ceil() as i32 + edge,
            right: ink.right().0.floor() as i32 - edge,
            bottom: ink.bottom().0.floor() as i32 - edge,
        };
        let b = item.bounds();
        let reach = (corner + border).ceil() as i32 + 1;
        let (left, top) = (b.left().0.floor() as i32, b.top().0.floor() as i32);
        let (right, bottom) = (b.right().0.ceil() as i32, b.bottom().0.ceil() as i32);
        let square = |x: i32, y: i32| RECT {
            left: x,
            top: y,
            right: x + reach,
            bottom: y + reach,
        };
        within(self.clip, inner)
            && [
                square(left, top),
                square(right - reach, top),
                square(left, bottom - reach),
                square(right - reach, bottom - reach),
            ]
            .iter()
            .all(|corner| !overlaps(corner, &self.clip))
    }

    fn match_kind<T: Ink>(&mut self, now: &[T], old: &[T]) {
        let by = point(ScaledPixels(self.by.0 as f32), ScaledPixels(self.by.1 as f32));
        let copied = Bounds::from_corners(
            point(
                ScaledPixels(self.copied.left as f32),
                ScaledPixels(self.copied.top as f32),
            ),
            point(
                ScaledPixels(self.copied.right as f32),
                ScaledPixels(self.copied.bottom as f32),
            ),
        );
        // A primitive moved into the copy compares with one now by their
        // masks within the copy only: there both show the same pixels,
        // whether the mask moved along, holds the whole clip or is cut by
        // its edge. Elsewhere its pixels are drawn (`scrolled`).
        let in_copy = |item: &T| {
            let mask = item.mask().intersect(&copied);
            (mask.size.width.0 > 0. && mask.size.height.0 > 0.).then(|| item.with_mask(mask))
        };
        // How an old primitive can appear now: in place (0) or moved (1).
        let form = |item: &T, kind: u8| match kind {
            0 => Some(*item),
            _ => item.shifted(by, true).and_then(|moved| in_copy(&moved)),
        };
        let mut used = vec![false; old.len()];
        let check = !self.fresh.is_empty();
        let fresh = |item: &T| check && is_fresh(item, self.fresh);
        // What each primitive now matches in the last frame, and how. A
        // scroll keeps the order of what it moves and of what stays, so the
        // match is most often the next unused primitive of the last frame, as
        // it was or moved: those few are compared directly first. Hashing
        // every form of every primitive cost more than the rest of a scroll
        // frame's matching.
        const WINDOW: usize = 8;
        let mut matches: Vec<Option<(usize, u8)>> = vec![None; now.len()];
        let mut next = 0;
        let mut unmatched = false;
        for (item, found) in now.iter().zip(&mut matches) {
            if fresh(item) {
                continue;
            }
            let own = bytes(item);
            let mut moved_own: Option<Option<T>> = None;
            for index in next..old.len().min(next + WINDOW) {
                if used[index] {
                    continue;
                }
                let candidate = &old[index];
                let kind = if bytes(candidate) == own {
                    Some(0)
                } else {
                    let moved_own = moved_own.get_or_insert_with(|| in_copy(item));
                    moved_own
                        .as_ref()
                        .zip(form(candidate, 1))
                        .is_some_and(|(own, seen)| bytes(own) == bytes(&seen))
                        .then_some(1)
                };
                if let Some(kind) = kind {
                    used[index] = true;
                    *found = Some((index, kind));
                    next = index + 1;
                    break;
                }
            }
            unmatched |= found.is_none();
        }
        // The rest by the bytes of each form, among what is still unused:
        // as it was (0) before moved (1).
        if unmatched {
            let mut heads = collections::FxHashMap::<u64, u32>::default();
            let mut entries: Vec<(u32, u8, u32)> = Vec::new();
            for (index, item) in old.iter().enumerate() {
                if used[index] {
                    continue;
                }
                for kind in [0, 1] {
                    if let Some(seen) = form(item, kind) {
                        let key = hash_bytes(bytes(&seen));
                        let next = heads.insert(key, entries.len() as u32).unwrap_or(u32::MAX);
                        entries.push((index as u32, kind, next));
                    }
                }
            }
            for (item, found) in now.iter().zip(&mut matches) {
                if found.is_some() || fresh(item) {
                    continue;
                }
                for (kind, own) in [(0u8, Some(*item)), (1, in_copy(item))] {
                    let Some(own) = own else {
                        continue;
                    };
                    let own = bytes(&own);
                    let mut at = heads.get(&hash_bytes(own)).copied().unwrap_or(u32::MAX);
                    while at != u32::MAX && found.is_none() {
                        let (index, entry, next) = entries[at as usize];
                        if entry == kind
                            && !used[index as usize]
                            && form(&old[index as usize], kind)
                                .is_some_and(|seen| bytes(&seen) == own)
                        {
                            used[index as usize] = true;
                            *found = Some((index as usize, kind));
                        }
                        at = next;
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
        }
        for (item, found) in now.iter().zip(matches) {
            let Some(rect) = self.rect(item) else {
                if let Some((index, _)) = found {
                    used[index] = true;
                }
                continue;
            };
            match found {
                Some((index, 0)) => {
                    used[index] = true;
                    // Over the copied pixels, or copied itself: one in the
                    // strip that comes into view has its old pixels copied
                    // next to it.
                    if overlaps(&rect, &self.copied)
                        || overlaps(&offset(rect, self.by), &self.copied)
                    {
                        let covers = item
                            .plain_fill()
                            .is_some_and(|(corner, border)| self.uniform_over_clip(item, corner, border));
                        self.fixed.push((item.order(), rect, covers));
                    }
                }
                Some((index, _)) => {
                    used[index] = true;
                    self.moved += 1;
                    self.lowest_moved = self.lowest_moved.min(item.order());
                    // The copy brings it only from inside the clip to inside
                    // the clip; any part elsewhere is drawn in both places.
                    for piece in subtract_rect(rect, self.clip) {
                        self.include(piece);
                    }
                    if let Some(old_rect) = self.rect(&old[index]) {
                        self.include_uncopied(old_rect);
                    }
                }
                None => self.include(rect),
            }
        }
        for (index, item) in old.iter().enumerate() {
            if !used[index]
                && let Some(rect) = self.rect(item)
            {
                // Gone or changed: where it was, but for what the copy
                // replaces, and where the copy puts it.
                self.include_uncopied(rect);
                self.include_copied(offset(rect, self.by));
            }
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

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Rows of a list in the clip (0, 20)–(200, 120), scrolled to `offset`,
    /// over a rounded card; each row its own color.
    fn list(offset: f32, card: Quad) -> Snapshot {
        let mask = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(20.)),
            size(ScaledPixels(200.), ScaledPixels(100.)),
        );
        let mut rows = vec![card];
        for index in 0..12 {
            let y = 20. + offset + index as f32 * 20.;
            rows.push(Quad {
                order: 2,
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(y)),
                    size(ScaledPixels(200.), ScaledPixels(20.)),
                ),
                content_mask: ContentMask { bounds: mask },
                background: hsla(index as f32 / 12., 0.5, 0.5, 1.).into(),
                ..Default::default()
            });
        }
        quads(rows)
    }

    fn card(corner: f32) -> Quad {
        Quad {
            order: 1,
            bounds: Bounds::new(
                point(ScaledPixels(-20.), ScaledPixels(0.)),
                size(ScaledPixels(240.), ScaledPixels(140.)),
            ),
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(-100.), ScaledPixels(-100.)),
                    size(ScaledPixels(400.), ScaledPixels(400.)),
                ),
            },
            background: hsla(0., 0., 1., 0.04).into(),
            corner_radii: Corners::all(ScaledPixels(corner)),
            ..Default::default()
        }
    }

    fn drawn(plan: &Scrolled) -> Vec<RECT> {
        let mut damage = plan.damage.clone();
        damage.sort_by_key(|r| (r.top, r.left));
        damage.dedup_by(|a, b| a == b);
        damage
    }

    #[::core::prelude::v1::test]
    fn a_scrolled_list_draws_only_the_rows_that_came_into_view() {
        let none = FreshTiles::default();
        let clip = rect(0, 20, 200, 120);
        let before = list(0., card(8.));
        let after = list(-10., card(8.));
        let plan = after
            .scrolled(&before, clip, (0, -10), 300, 200, &none)
            .expect("copied pixels");
        assert_eq!(plan.copied, rect(0, 20, 200, 110));
        // The card's corners lie outside the clip: one color under the rows.
        assert_eq!(drawn(&plan), vec![rect(0, 110, 200, 120)]);
        assert!(plan.moved > 0);
        // Its corner reaching into the clip would show at a moved place.
        let near = card(30.);
        let plan = list(-10., near)
            .scrolled(&list(0., near), clip, (0, -10), 300, 200, &none)
            .expect("copied pixels");
        assert!(drawn(&plan).len() > 1);
    }

    #[::core::prelude::v1::test]
    fn rows_listed_in_another_order_match_as_rows_in_order_do() {
        let none = FreshTiles::default();
        let clip = rect(0, 20, 200, 120);
        let before = list(0., card(8.));
        let after = list(-10., card(8.));
        let in_order = after
            .scrolled(&before, clip, (0, -10), 300, 200, &none)
            .expect("copied pixels");
        // The same rows, listed back to front this frame: past the few
        // compared in order, each is found by its bytes.
        let mut reversed = after.clone();
        reversed.quads[1..].reverse();
        let plan = reversed
            .scrolled(&before, clip, (0, -10), 300, 200, &none)
            .expect("copied pixels");
        assert_eq!(plan.moved, in_order.moved);
        assert_eq!(drawn(&plan), drawn(&in_order));
    }

    #[::core::prelude::v1::test]
    fn a_fixed_draw_in_the_strip_coming_into_view_is_drawn_where_its_copy_lands() {
        let none = FreshTiles::default();
        let clip = rect(0, 20, 200, 120);
        // A divider fixed at the bottom of the list, in the strip the move
        // brings into view: the copy carries its old pixels to 105..110.
        let mut divider = card(0.);
        divider.order = 3;
        divider.bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(115.)),
            size(ScaledPixels(200.), ScaledPixels(5.)),
        );
        let mut before = list(0., card(8.));
        before.quads.push(divider);
        let mut after = list(-10., card(8.));
        after.quads.push(divider);
        let damage = drawn(
            &after
                .scrolled(&before, clip, (0, -10), 300, 200, &none)
                .expect("copied pixels"),
        );
        assert!(damage.contains(&rect(0, 105, 200, 110)), "{damage:?}");
    }

    #[::core::prelude::v1::test]
    fn a_gradient_does_not_move_with_its_bounds() {
        let by = point(ScaledPixels(0.), ScaledPixels(10.));
        let mut quad = card(0.);
        assert!(quad.shifted(by, true).is_some());
        // Dithered by screen position.
        quad.background = linear_gradient(
            90.,
            linear_color_stop(gpui::black(), 0.),
            linear_color_stop(gpui::white(), 1.),
        );
        assert!(quad.shifted(by, true).is_none());
    }

    #[::core::prelude::v1::test]
    fn a_changed_row_draws_where_it_is_and_where_the_copy_puts_its_old_pixels() {
        let none = FreshTiles::default();
        let clip = rect(0, 20, 200, 120);
        let before = list(0., card(8.));
        let mut after = list(-10., card(8.));
        // The row at 50..70 now (40..60 + 10 before the move) changes color.
        after.quads[3].background = hsla(0.9, 1., 0.5, 1.).into();
        let plan = after
            .scrolled(&before, clip, (0, -10), 300, 200, &none)
            .expect("copied pixels");
        let damage = drawn(&plan);
        assert!(damage.contains(&rect(0, 50, 200, 70)), "{damage:?}");
        assert!(damage.contains(&rect(0, 110, 200, 120)), "{damage:?}");
        // A fixed strip over the rows is drawn at both places.
        let mut strip = card(0.);
        strip.order = 3;
        strip.bounds = Bounds::new(
            point(ScaledPixels(190.), ScaledPixels(20.)),
            size(ScaledPixels(10.), ScaledPixels(30.)),
        );
        let mut before = list(0., card(8.));
        before.quads.push(strip);
        let mut after = list(-10., card(8.));
        after.quads.push(strip);
        let damage = drawn(
            &after
                .scrolled(&before, clip, (0, -10), 300, 200, &none)
                .expect("copied pixels"),
        );
        assert!(damage.contains(&rect(190, 20, 200, 50)), "{damage:?}");
        assert!(damage.contains(&rect(190, 20, 200, 40)), "{damage:?}");
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
        // Shadows are culled by where their blur reaches; paths are not.
        assert!(local_scene(
            &mut Scene::default(),
            &scene,
            &[PrimitiveBatch::Shadows(0..0)],
            &[rect]
        ));
        assert!(!local_scene(
            &mut Scene::default(),
            &scene,
            &[PrimitiveBatch::Paths(0..0)],
            &[rect]
        ));
    }

    #[::core::prelude::v1::test]
    fn a_shadow_reaches_three_blur_radii_around_its_bounds() {
        let shadow = Shadow {
            order: 1,
            blur_radius: ScaledPixels(4.),
            bounds: Bounds::new(
                point(ScaledPixels(50.), ScaledPixels(50.)),
                size(ScaledPixels(20.), ScaledPixels(10.)),
            ),
            corner_radii: Corners::default(),
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(ScaledPixels(0.), ScaledPixels(0.)),
                    size(ScaledPixels(200.), ScaledPixels(200.)),
                ),
            },
            color: gpui::black(),
            element_bounds: Bounds::default(),
            element_corner_radii: Corners::default(),
            inset: 0,
            pad: 0,
        };
        assert_eq!(
            ink_rect(shadow.ink(), 200, 200),
            Some(RECT {
                left: 38,
                top: 38,
                right: 82,
                bottom: 72,
            })
        );
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
