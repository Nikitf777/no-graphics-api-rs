# no-graphics-api-rs

Rust bindings and Rust-native helpers for [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI), a
thin GPU abstraction over Vulkan (Windows/Linux) and Metal 4 (macOS/iOS).

| Crate | Contents |
| --- | --- |
| [`no-graphics-api-sys`](crates/no-graphics-api-sys) | Low-level `unsafe` bindings: bindgen over a C ABI shim compiled against the C++ headers |
| [`no-graphics-api-utility`](crates/no-graphics-api-utility) | Safe Rust ports of the host-side `NoGraphicsAPIUtility` helpers (allocators, delete queue, upload queue, texture upload, shader types) |

Upstream commit: `ffd9f2a` (vendored under `crates/no-graphics-api-sys/vendor/NoGraphicsAPI`).

The split mirrors the upstream design: the bindings are a one-to-one mirror of the C++ API and stay
`unsafe`, while the utility layer is ordinary safe Rust on top of them — no second FFI surface, no
trampolines, and closures instead of C function pointers plus context.

## Build requirements

* A C++20 compiler (`cc` crate) and `libclang` (bindgen).
* Vulkan headers (`vulkan/vulkan.h`) on non-Apple targets, and the Vulkan loader at build and run time.
  `VULKAN_SDK` is honored.
* Apple targets compile the Metal backend and link `Metal`, `Foundation`, and `QuartzCore`; they require
  macOS/iOS 26 and Apple GPU family 7 or newer.

`no-graphics-api-sys` compiles the vendored sources by default. Set `NO_GRAPHICS_API_SOURCE` to a
NoGraphicsAPI checkout to compile that instead. `no-graphics-api-utility` is pure Rust.

## Example

```rust
use no_graphics_api_sys as gpu;
use no_graphics_api_utility::{DeleteQueue, HeapAllocator};

unsafe {
    let mut init = gpu::DeviceInit::default();
    gpu::create_device(&gpu::DeviceDesc::default(), &mut init);
    assert_eq!(init.error, gpu::Error::none);

    let mut heap = gpu::GpuHeap::default();
    gpu::create_gpu_heap(init.device, 1 << 20, gpu::MemoryType::cpu_visible, &mut heap);

    // Variable-size suballocation of the heap's CPU/GPU address pair.
    let mut allocator = HeapAllocator::new(heap.range, 64);
    let upload = allocator.allocate(256).expect("heap range");
    let range = gpu::GpuRange { gpu: upload.range.gpu.cast(), size: u64::from(upload.range.size) };
    assert_eq!(range.size, 256);

    // Deferred destruction with a real closure; runs once the timeline retires it.
    let semaphore = gpu::create_timeline_semaphore(init.device, 0);
    let mut delete_queue = DeleteQueue::new(semaphore, 16);
    delete_queue.defer(1, || eprintln!("frame 1 retired"));

    gpu::wait_idle(init.device);
    delete_queue.drain();
    allocator.free(upload);
    gpu::destroy_gpu_heap(&heap);
    gpu::destroy_timeline_semaphore(semaphore);
    gpu::destroy_device(init.device);
}
```

## Testing

```sh
cargo test --workspace
```

Device-backed tests create a headless device and skip when no Vulkan device is available. The
allocator, layout, and delete-queue logic tests need no GPU.

## Updating upstream

```sh
scripts/sync-vendor.sh /path/to/NoGraphicsAPI
cargo test --workspace
```

The C++ shim is compiled against the same headers bindgen parses, so upstream signature or layout
changes surface as compile errors instead of silent ABI drift. Ported utility code lives in
`crates/no-graphics-api-utility/src` and must be re-checked by hand when upstream changes.
