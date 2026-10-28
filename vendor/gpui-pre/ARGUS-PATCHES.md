# Argus patches

Source: crates.io `gpui-pre` 0.3.9, based on Zed revision `93d745668805029476e6d7fbb19652116fed51de`. The upstream Apache-2.0 license and package metadata are retained.

The vendored package omits standalone examples and their media, the package-local lockfile and the unnormalized manifest. The small EXIF fixture referenced by upstream source tests is retained. Source, build resources, license and upstream documentation remain available.

Local changes:

- A position-only notification updates the screen-dependent state and notifies bounds observers without invalidating the entire local scene. Size and scale changes still use `bounds_changed`, including the explicit hidden-start viewport synchronization.
- Native caption hit testing uses the current platform pointer position against the last rendered hitboxes, rather than reusing the preceding mouse event's hit result.
- Paint-cache replay retains native window-control hitboxes along with cursor styles and input handlers. Cached title bars keep their drag, minimize, maximize, and close regions on every frame.
- `paint_image_subpixel` retains fractional device coordinates for continuously translated raster layers and clips through the existing content mask. Regular image controls retain their original pixel snapping.
- Decorative images and bars (optionally with rounded corners) can update their bounds in a clean presented scene. Handles expire on full draws, reserve fixed paint layers, and validate window identity, atlas lifetime, clip bounds and primitive type. Cached paint operations are updated with their corresponding scene entries; dirty scenes and interactions use normal drawing.
- Scene revisions and the earliest animated draw order let renderers retain immutable pixels before the animated primitives. A complete paint invalidates the prefix, including when cached views replay into it.
- The runnable counter uses `AtomicUsize::try_update`, the current name of `fetch_update` on the workspace Rust toolchain. Ordering and checked subtraction are unchanged.
- Text measurement reuses matching wrap/truncation constraints and unwrapped text that fits the new width. Truncated or wrapped results cannot answer wider intrinsic probes. Line wrappers are acquired only for actual truncation, and decoration storage follows the number of style runs.
- Inherited text styles cache their resolved prefix, and text without background decorations skips the empty background paint pass.
- Taffy 0.14 uses its leaf-layout adapter for measured elements. Opt-in frame diagnostics report layout node counts, measurement time and cleanup separately; normal rendering does not enable the timers.

The owned-window probe exercises normal startup, caption movement, resize, frame timing, and foreground isolation. Remove these patches when an upstream version provides equivalent behavior.
