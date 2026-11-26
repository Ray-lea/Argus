//! Argus patch: visual and live transforms, which move, scale, fade and clip
//! what is painted inside them without changing layout, and rules whose
//! stroke width is snapped after the transforms around them.
use super::*;
use crate::size;

/// A translation, scale, opacity and clip for everything painted inside it
/// (see [`crate::Window::with_visual_transform`],
/// [`crate::Window::with_live_transform`] and [`crate::Window::with_visual_clip`]).
/// It only changes what is drawn: layout, hit testing, element bounds and
/// content masks stay as laid out. A device position `p` is drawn at
/// `p * scale + offset`, each axis with its own scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualTransform {
    /// Device pixels to move by (after scaling).
    pub offset: Point<ScaledPixels>,
    /// Multiplies horizontal positions and widths about the device origin.
    pub scale_x: f32,
    /// Multiplies vertical positions and heights about the device origin.
    pub scale_y: f32,
    /// Multiplies border widths, corner radii, blur radii and underline
    /// thickness: the scale for a zoom, 1 for a stretch that resizes boxes
    /// without thickening their borders.
    pub lengths: f32,
    /// Multiplies the opacity of everything inside.
    pub opacity: f32,
    /// Device-pixel bounds nothing inside is drawn outside of, placed in the
    /// coordinates around the transform (its own offset and scale do not move it).
    pub clip: Option<Bounds<ScaledPixels>>,
}

fn snap_edges(rect: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
    let snap = |v: ScaledPixels| ScaledPixels(crate::util::round_half_toward_zero(v.0));
    Bounds::from_corners(rect.origin.map(snap), rect.bottom_right().map(snap))
}

impl VisualTransform {
    /// A move by `offset` (device pixels) at `opacity`.
    pub fn moved(offset: Point<ScaledPixels>, opacity: f32) -> Self {
        Self {
            offset,
            scale_x: 1.,
            scale_y: 1.,
            lengths: 1.,
            opacity,
            clip: None,
        }
    }

    fn scaled(&self) -> bool {
        self.scale_x != 1. || self.scale_y != 1.
    }

    /// `inner` inside `self`.
    fn then(self, inner: Self) -> Self {
        Self {
            offset: point(
                self.offset.x + inner.offset.x * self.scale_x,
                self.offset.y + inner.offset.y * self.scale_y,
            ),
            scale_x: self.scale_x * inner.scale_x,
            scale_y: self.scale_y * inner.scale_y,
            lengths: self.lengths * inner.lengths,
            opacity: self.opacity * inner.opacity,
            clip: match (self.clip, inner.clip.map(|clip| self.bounds(clip))) {
                (Some(outer), Some(inner)) => Some(outer.intersect(&inner)),
                (outer, inner) => outer.or(inner),
            },
        }
    }

    pub(super) fn bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        Bounds {
            origin: self.point(bounds.origin),
            size: size(
                bounds.size.width * self.scale_x,
                bounds.size.height * self.scale_y,
            ),
        }
    }

    fn point(&self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        crate::point(
            point.x * self.scale_x + self.offset.x,
            point.y * self.scale_y + self.offset.y,
        )
    }

    fn length(&self, length: ScaledPixels) -> ScaledPixels {
        length * self.lengths
    }

    fn stroke(&self, width: ScaledPixels) -> ScaledPixels {
        ScaledPixels(crate::util::round_stroke_to_device_pixel(
            width.0,
            self.lengths,
        ))
    }

    fn corners(&self, corners: Corners<ScaledPixels>) -> Corners<ScaledPixels> {
        Corners {
            top_left: self.length(corners.top_left),
            top_right: self.length(corners.top_right),
            bottom_right: self.length(corners.bottom_right),
            bottom_left: self.length(corners.bottom_left),
        }
    }

    fn mask(&self, mask: ContentMask<ScaledPixels>) -> ContentMask<ScaledPixels> {
        let bounds = self.bounds(mask.bounds);
        ContentMask {
            bounds: match self.clip {
                Some(clip) => bounds.intersect(&clip),
                None => bounds,
            },
        }
    }

    /// The primitive as drawn: moved, scaled and faded. Unscaled glyphs and
    /// images move by whole device pixels, where a fresh paint would have
    /// snapped them; scaled ones keep their exact place, so that they do not
    /// jump a pixel at a time while the scale changes.
    pub(super) fn apply(&self, primitive: &Primitive) -> Primitive {
        let snapped = if self.scaled() {
            *self
        } else {
            VisualTransform {
                offset: point(
                    ScaledPixels(self.offset.x.0.round()),
                    ScaledPixels(self.offset.y.0.round()),
                ),
                ..*self
            }
        };
        let opacity = self.opacity;
        match primitive {
            Primitive::Shadow(shadow) => Primitive::Shadow(Shadow {
                bounds: self.bounds(shadow.bounds),
                content_mask: self.mask(shadow.content_mask),
                element_bounds: self.bounds(shadow.element_bounds),
                corner_radii: self.corners(shadow.corner_radii),
                element_corner_radii: self.corners(shadow.element_corner_radii),
                blur_radius: self.length(shadow.blur_radius),
                color: shadow.color.opacity(opacity),
                ..*shadow
            }),
            Primitive::Quad(quad) => {
                let mut placed = Quad {
                    bounds: self.bounds(quad.bounds),
                    content_mask: self.mask(quad.content_mask),
                    background: quad.background.opacity(opacity),
                    border_color: quad.border_color.opacity(opacity),
                    corner_radii: self.corners(quad.corner_radii),
                    ..*quad
                };
                if quad.border_widths != Edges::default() {
                    // Border strips share a box and meet at clip edges. Snap both
                    // after composing all transforms so adjacent strips still meet.
                    placed.bounds = snap_edges(placed.bounds);
                    placed.content_mask.bounds = snap_edges(placed.content_mask.bounds);
                    placed.border_widths = quad.border_widths.map(|w| self.stroke(*w));
                }
                Primitive::Quad(placed)
            }
            Primitive::Path(path) => {
                let mut path = path.clone();
                path.bounds = self.bounds(path.bounds);
                path.content_mask = self.mask(path.content_mask);
                for vertex in &mut path.vertices {
                    vertex.xy_position = self.point(vertex.xy_position);
                    vertex.content_mask = self.mask(vertex.content_mask.clone());
                }
                path.color = path.color.opacity(opacity);
                Primitive::Path(path)
            }
            Primitive::Underline(underline) => Primitive::Underline(Underline {
                bounds: self.bounds(underline.bounds),
                content_mask: self.mask(underline.content_mask),
                color: underline.color.opacity(opacity),
                thickness: self.length(underline.thickness),
                ..*underline
            }),
            Primitive::MonochromeSprite(sprite) => Primitive::MonochromeSprite(MonochromeSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                color: sprite.color.opacity(opacity),
                transformation: snapped.matrix(sprite.transformation),
                ..*sprite
            }),
            Primitive::SubpixelSprite(sprite) => Primitive::SubpixelSprite(SubpixelSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                color: sprite.color.opacity(opacity),
                transformation: snapped.matrix(sprite.transformation),
                ..*sprite
            }),
            Primitive::PolychromeSprite(sprite) => Primitive::PolychromeSprite(PolychromeSprite {
                bounds: snapped.bounds(sprite.bounds),
                content_mask: self.mask(sprite.content_mask),
                corner_radii: self.corners(sprite.corner_radii),
                opacity: sprite.opacity * opacity,
                ..*sprite
            }),
            Primitive::Surface(surface) => {
                let mut surface = surface.clone();
                surface.bounds = snapped.bounds(surface.bounds);
                surface.content_mask = self.mask(surface.content_mask);
                Primitive::Surface(surface)
            }
        }
    }

    /// A sprite's matrix for its transformed bounds. The matrix maps device
    /// positions (`p * rotation_scale + translation`) after the bounds place
    /// them, so its translation takes the scale and absorbs the move it would
    /// rotate: drawn, `p` lands at `(p * rotation_scale + translation) * scale
    /// + offset` (exactly for unrotated sprites, the only ones scaled unevenly).
    fn matrix(&self, matrix: TransformationMatrix) -> TransformationMatrix {
        let [[a, b], [c, d]] = matrix.rotation_scale;
        let (x, y) = (self.offset.x.0, self.offset.y.0);
        let [tx, ty] = matrix.translation;
        TransformationMatrix {
            rotation_scale: matrix.rotation_scale,
            translation: [
                tx * self.scale_x + x - (x * a + y * c),
                ty * self.scale_y + y - (x * b + y * d),
            ],
        }
    }
}

/// What a [`LiveTransform`] draws its contents with, about the anchor it is
/// painted with: a point `p` lands at `anchor + (p - anchor) * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveTransformValue {
    /// Logical pixels to move by.
    pub offset: Point<Pixels>,
    /// Multiplies horizontal distances from the anchor.
    pub scale_x: f32,
    /// Multiplies vertical distances from the anchor.
    pub scale_y: f32,
    /// Multiplies border widths, corner radii, blur radii and underline
    /// thickness (see [`VisualTransform::lengths`]).
    pub lengths: f32,
    /// Multiplies the opacity of everything inside.
    pub opacity: f32,
}

impl Default for LiveTransformValue {
    fn default() -> Self {
        Self {
            offset: Point::default(),
            scale_x: 1.,
            scale_y: 1.,
            lengths: 1.,
            opacity: 1.,
        }
    }
}

impl LiveTransformValue {
    /// Scaled by `scale` about the anchor, borders and text included (a zoom), at `opacity`.
    pub fn zoom(scale: f32, opacity: f32) -> Self {
        Self {
            scale_x: scale,
            scale_y: scale,
            lengths: scale,
            opacity,
            ..Self::default()
        }
    }

    /// Whether it draws its contents as laid out.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    fn visual(&self, anchor: Point<ScaledPixels>, scale_factor: f32) -> VisualTransform {
        let finite = |value: f32| if value.is_finite() { value } else { 1. };
        let (scale_x, scale_y) = (finite(self.scale_x), finite(self.scale_y));
        VisualTransform {
            offset: point(
                anchor.x * (1. - scale_x) + ScaledPixels(self.offset.x.0 * scale_factor),
                anchor.y * (1. - scale_y) + ScaledPixels(self.offset.y.0 * scale_factor),
            ),
            scale_x,
            scale_y,
            lengths: finite(self.lengths),
            opacity: self.opacity.clamp(0., 1.),
            clip: None,
        }
    }
}

/// A visual transform whose value is read whenever the scene is built,
/// including when a cached view painted inside it replays: changing the value
/// and drawing again moves, scales and fades the contents without rendering
/// the view that painted them (see [`crate::Window::with_live_transform`]).
#[derive(Clone, Default)]
pub struct LiveTransform(std::rc::Rc<std::cell::Cell<LiveTransformValue>>);

impl LiveTransform {
    /// A live transform drawing its contents as laid out.
    pub fn new(value: LiveTransformValue) -> Self {
        Self(std::rc::Rc::new(std::cell::Cell::new(value)))
    }

    /// The value the next scene built uses.
    pub fn set(&self, value: LiveTransformValue) {
        self.0.set(value);
    }

    /// The value scenes are built with now.
    pub fn get(&self) -> LiveTransformValue {
        self.0.get()
    }
}

impl Debug for LiveTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

#[expect(missing_docs)]
impl Scene {
    /// Moves and fades everything inserted until the matching
    /// [`Self::pop_transform`]. The operations keep the untransformed
    /// primitives, so replaying them follows the transforms then in effect.
    pub fn push_transform(&mut self, transform: VisualTransform) {
        let composed = match self.transform_stack.last() {
            Some(outer) => outer.then(transform),
            None => transform,
        };
        self.transform_stack.push(composed);
        self.paint_operations
            .push(PaintOperation::PushTransform(transform));
    }

    /// Like [`Self::push_transform`] with the current value of `transform`
    /// about `anchor`; a replay of the operations reads its value again.
    pub fn push_live_transform(
        &mut self,
        transform: LiveTransform,
        anchor: Point<ScaledPixels>,
        scale_factor: f32,
    ) {
        let visual = transform.get().visual(anchor, scale_factor);
        let composed = match self.transform_stack.last() {
            Some(outer) => outer.then(visual),
            None => visual,
        };
        self.transform_stack.push(composed);
        self.paint_operations
            .push(PaintOperation::PushLiveTransform {
                transform,
                anchor,
                scale_factor,
            });
    }

    pub fn pop_transform(&mut self) {
        self.transform_stack.pop();
        self.paint_operations.push(PaintOperation::PopTransform);
    }

    /// Inserts a filled rule in device coordinates. Keep its original geometry
    /// for replay so the final transform determines position and stroke width.
    pub fn insert_rule(&mut self, axis: crate::Axis, quad: Quad) {
        let mut placed = quad;
        if let Some(transform) = self.transform_stack.last() {
            placed.bounds = snap_edges(transform.bounds(quad.bounds));
            match axis {
                crate::Axis::Horizontal => {
                    placed.bounds.size.height = transform.stroke(quad.bounds.size.height)
                }
                crate::Axis::Vertical => {
                    placed.bounds.size.width = transform.stroke(quad.bounds.size.width)
                }
            }
            placed.content_mask = transform.mask(quad.content_mask);
            placed.background = quad.background.opacity(transform.opacity);
        }
        self.place_primitive(Primitive::Quad(placed), false);
        self.paint_operations.push(PaintOperation::Rule(axis, quad));
    }
}
