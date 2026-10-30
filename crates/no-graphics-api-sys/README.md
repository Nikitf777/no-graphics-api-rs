# no-graphics-api-sys

Low-level `unsafe` Rust bindings for [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI), a thin
GPU abstraction over Vulkan (Windows/Linux) and Metal 4 (macOS/iOS).

This is the bindings half of the `no-graphics-api-rs` workspace. The bindings are a one-to-one mirror
of the C++ API: opaque handle pointers, `#[repr(C)]` descriptor aggregates, plain enums, and free
functions. Nothing is validated, reference counted, or wrapped in safe abstractions — every function
is `unsafe` and must be called under the contract documented in the upstream headers. Safe helpers
live in [`no-graphics-api-utility`](../no-graphics-api-utility).

Upstream commit: `ffd9f2a` (vendored under `vendor/NoGraphicsAPI`, see `vendor/NoGraphicsAPI/UPSTREAM.md`).

## Layout

| Path | Contents |
| --- | --- |
| `csi/shim.h`, `csi/shim.cpp` | C ABI shim over the C++ entry points |
| `build.rs` | Compiles the shim plus the vendored backend with `cc`, generates bindings with bindgen |
| `src/lib.rs` | Re-exports, `Span`/`ByteSpan` helpers, crate-level safety notes |
| `src/defaults.rs` | `Default` impls mirroring the C++ member initializers |
| `tests/smoke.rs` | Headless device, heap, texture, descriptor, render pass, and submit round trip |
| `vendor/NoGraphicsAPI` | Upstream headers and backend sources |

## Build requirements

* A C++20 compiler (`cc` crate) and `libclang` (bindgen).
* Vulkan headers, i.e. `vulkan/vulkan.h` on non-Apple targets.
* The Vulkan loader at build and run time (`vulkan` / `vulkan-1`). `VULKAN_SDK` is honored.
* Apple targets compile `src/NoGraphicsAPIMetal.mm` and link `Metal`, `Foundation`, and `QuartzCore`;
  they require macOS/iOS 26 and Apple GPU family 7 or newer.

By default the vendored sources are compiled. Point `NO_GRAPHICS_API_SOURCE` at any other
NoGraphicsAPI checkout to compile that instead.

## Usage

```rust
use no_graphics_api_sys as gpu;

unsafe {
    let mut init = gpu::DeviceInit::default();
    gpu::create_device(&gpu::DeviceDesc::default(), &mut init);
    assert_eq!(init.error, gpu::Error::none);

    let caps = &*gpu::get_device_caps(init.device);
    println!("{:?}", caps.device_name_cstr().unwrap());

    gpu::wait_idle(init.device);
    gpu::destroy_device(init.device);
}
```

* Types and enumerators keep their C++ names: `TextureDesc`, `Format::rgba8_unorm`,
  `Stage::fragment`, `TextureUsage::sampled | TextureUsage::storage`.
* Functions keep their C++ names too (`create_device`, `begin_render_pass`, `draw_indexed`). The raw
  bindgen output with its `nga_` prefix lives in `no_graphics_api_sys::ffi`
  (`ffi::nga_create_device`, `ffi::gpu::DeviceDesc`).
* Descriptors have `Default` impls that match the C++ defaults, so use struct update syntax:
  `TextureDesc { usage: gpu::TextureUsage::storage, ..Default::default() }`.
* Array parameters are `Span<T>` fields. Build them with `Span::from_slice(&mut slice)` or
  `Span::new(ptr, len)`; they borrow for the duration of the call.
* `ShaderStage::code` takes a `ByteSpan` (precompiled SPIR-V words on Vulkan, a metallib on Metal).
  `entry_point` defaults to a `"main"` C string.

## Linux Wayland presentation (local patch)

Upstream NoGraphicsAPI creates a Vulkan surface only on Windows (Win32) and macOS (Metal layer); on
Linux `create_device` returns `Error::unsupported` for any window. This crate applies a small local
patch, `vendor/patches/0001-wayland-presentation.patch`, until that lands upstream:

| Field | Meaning |
| --- | --- |
| `DeviceDesc::display` | `wl_display*` on Wayland; unused on Windows/macOS |
| `DeviceDesc::window` | `wl_surface*` on Wayland (HWND / CAMetalLayer* elsewhere) |
| `DeviceDesc::swapchain_extent` | Drawable size for surfaces without a fixed extent (Wayland reports none) |
| `set_swapchain_extent(device, extent)` | Follows window resizes on those surfaces; a zero component keeps the current size |

The `wayland` feature (on by default) compiles the patch and requires `<wayland-client.h>` on Linux;
use `default-features = false` for headless-only builds without Wayland headers. `scripts/sync-vendor.sh`
re-applies the patches after re-syncing (`--no-patch` skips them) and `tests/presentation.rs` in the
examples crate presents real frames under a headless weston.

## Safety

The upstream documentation is authoritative. In short:

* Resource destruction is immediate. Destroy a resource only after the GPU frame using it completed
  (`wait_timeline` / `wait_idle`), never while it is bound or recorded.
* `(device, queue_index)` and each command pool (including recording its buffers) are externally
  synchronized. Device idle/destruction requires exclusive access. Command pools must be destroyed
  before the device.
* Every submission needs a completion `TimelinePoint`; a non-empty `acquire` must be submitted with
  `submit_and_present` on queue zero. `acquire` asserts when the device has no swapchain (headless).
* Root data pointers are 16-byte-aligned GPU addresses owned by the application; they must stay
  valid and unchanged until the GPU completes.
* `write_timestamp` destinations are ordinary CPU memory that must outlive the pool reset and be
  read with `read_timestamps`.
* Handles are raw pointers, so the types are neither `Send` nor `Sync`. Add your own `unsafe impl`
  only where the external synchronization rules above are upheld.

## Updating upstream

```sh
scripts/sync-vendor.sh /path/to/NoGraphicsAPI
cargo test -p no-graphics-api-sys
```
