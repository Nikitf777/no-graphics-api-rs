# no-graphics-api-examples

Rust ports of the three [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI) examples. Each one opens
a `winit` window, creates a swapchain-backed device, and renders until the window closes — the same
flow as the upstream C++ examples.

| Example | Upstream | Exercises |
| --- | --- | --- |
| `triangle` | `examples/triangle` | graphics PSO, clear render pass, `draw`, two frames in flight |
| `cube` | `examples/cube` | depth buffer, descriptor heaps, texture upload, root data, bump/texture allocators, indexed draw, resize |
| `deferred` | `examples/deferred_renderer` | compute dispatch, mesh-shader G-buffer, deferred lighting, delete queue, multi-target passes, resize |

Both loops mirror upstream: wait when the frame budget is exhausted, `acquire`, record, then
`submit_and_present`; the rotate/upload timing and per-frame root data are the same.

## Running

```sh
cargo run -p no-graphics-api-examples --example triangle
cargo run -p no-graphics-api-examples --example cube
cargo run -p no-graphics-api-examples --example deferred
```

Set `NGA_EXAMPLE_FRAMES=<n>` to exit after `n` frames (useful for smoke runs and CI).

Platform support follows the API: **Wayland** on Linux (the vendored patch has no X11 surface
support), **Win32** on Windows, and macOS would need a `CAMetalLayer` created over the `NSView`, which
this crate does not wire up yet.

## Shaders

`shaders/*.spv` is committed so building does not require the Slang toolchain. After editing a
`.slang` or shared header, regenerate with the same flags upstream's
`examples/NoGraphicsAPI_slang.cmake` uses:

```sh
crates/no-graphics-api-examples/scripts/build-shaders.sh   # needs slangc and spirv-val
```

Math uses `glam` and converts to the upstream `shader_types` layouts at the API boundary (row-major
matrices, 4-byte alignment). `shaders/lunarg_logo_256x256.rgba8` is the vkcube texture, Apache-2.0;
see `shaders/NOTICE.md` and `shaders/LICENSE-Apache-2.0.txt`.

## Tests

`tests/presentation.rs` exercises the Wayland presentation patch: it starts a private headless
`weston --renderer=gl`, creates a `wl_surface`, and presents frames through
`acquire`/`submit_and_present`, checking the drawable extent and that frames reach the compositor.
It skips when `weston` is unavailable.
