# Argus patches

Source: crates.io `gpui-pre-windows` 0.3.9, Zed revision `93d745668805029476e6d7fbb19652116fed51de`. The upstream Apache-2.0 license and package metadata are retained.

The package-local lockfile and unnormalized manifest are omitted; dependency resolution uses the workspace lockfile.

`src/directx_renderer.rs` batches per-draw constant parameters into reusable 64 KiB buffers. Devices supporting D3D11.1 constant-buffer offsets bind aligned ranges instead of repeatedly mapping a buffer for every draw. Unchanged batch layouts reuse the uploaded parameters. Device recreation clears these resources; drivers without offsetting retain the original binding path. Shaders, draw order, blending, antialiasing and sampling remain unchanged.

For bounds-only animations, the immutable prefix of the scene is copied as exact pixels from a cached texture. Batches at or after the first animated order still use the original rendering path. Complete paints, viewport changes and device recreation invalidate the cache; scenes with external surfaces use full rendering. Copying preserves output-merger state between batches. `ARGUS_GPUI_DISABLE_SCENE_CACHE` disables the pixel cache for controlled rendering comparisons.

Batches crossing the first animated draw order are split without reordering primitives. Retained frames restore the full animated masks, redraw the suffix and call `Present1` for that region; shrinking bars and translated images therefore erase their previous pixels. The dirty region is fully reconstructed on every frame, including intersections with earlier frames of the flip-sequential swap chain. Full draws and CPU readback still reconstruct the complete target; suffix shadows fall back to full presentation. See [DXGI incremental presentation](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/dxgi-1-2-presentation-improvements). Diagnostics include retained batches and dirty-region area.

The alignment and capability checks follow [VSSetConstantBuffers1](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_1/nf-d3d11_1-id3d11devicecontext1-vssetconstantbuffers1) and [D3D11_FEATURE_DATA_D3D11_OPTIONS](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_feature_data_d3d11_options). `ARGUS_GPUI_DIAGNOSTICS` reports the selected capability for owned tests. Remove the patch when equivalent behavior is available upstream.
