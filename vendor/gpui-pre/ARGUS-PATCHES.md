# Argus patches

Source: crates.io `gpui-pre` 0.3.9, based on Zed revision `93d745668805029476e6d7fbb19652116fed51de`. The upstream Apache-2.0 license and package metadata are retained.

Changes are confined to `src/window.rs` and `src/profiler/journal.rs`:

- A position-only notification updates the screen-dependent state and notifies bounds observers without invalidating the entire local scene. Size and scale changes still use `bounds_changed`, including the explicit hidden-start viewport synchronization.
- Native caption hit testing uses the current platform pointer position against the last rendered hitboxes, rather than reusing the preceding mouse event's hit result.
- Paint-cache replay retains native window-control hitboxes along with cursor styles and input handlers. Cached title bars keep their drag, minimize, maximize, and close regions on every frame.

The owned-window probe exercises normal startup, caption movement, resize, frame timing, and foreground isolation. Remove these patches when an upstream version provides equivalent behavior.

The vendored package omits standalone examples and their media, the package-local lockfile and the unnormalized manifest. The small EXIF fixture referenced by upstream source tests is retained. Source, build resources, license and upstream documentation remain available.

The runnable counter uses `AtomicUsize::try_update`, the current name of `fetch_update` on the workspace Rust toolchain. Ordering and checked subtraction are unchanged.
