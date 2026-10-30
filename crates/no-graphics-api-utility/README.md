# no-graphics-api-utility

Safe Rust ports of the host-side [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI) utility
helpers, built on top of [`no-graphics-api-sys`](../no-graphics-api-sys).

These are ports, not bindings. Upstream `NoGraphicsAPIUtility` is ordinary host-side policy code with
no driver calls, so reimplementing it in Rust removes an entire FFI surface: no opaque class handles,
no constructor/destructor pairs, no C-callback trampolines, and `unsafe` stays confined to the
bindings crate.

## Implemented

| Module | Upstream | Notes |
| --- | --- | --- |
| `shader_types` | `shader_types.h` | CPU/GPU shared POD types, byte-compatible with the Slang declarations |
| `bump_allocator` | `bump_allocator.hpp` | 16-byte bump pointer, lock-free `allocate_atomic` |
| `heap_allocator` | `heap_allocator.hpp` | Two-level bin allocator with neighbor coalescing |
| `texture_allocator` | `texture_allocator.hpp` | Places textures into a `TextureHeap` |
| `delete_queue` | `delete_queue.hpp` | Timeline-retired destructors as plain `FnOnce` closures |
| `upload_queue` | `upload_queue.hpp` | Staging ring, batch retirement, timestamps, submission handoff, compute callbacks |
| `texture_upload` | `texture_upload.hpp` | Tiled, granularity-aware region splitting into the upload ring |

Not ported:

* `math.hpp` — an operator library over `shader_types`. Use a Rust math crate (`glam`, `nalgebra`, …)
  and convert at the edge. Keep the `shader_types` layouts for anything a shader reads: some Rust math
  types use stricter alignment (`glam::Mat4` is 16-byte aligned, upstream `float4x4` is 4), so never
  embed them directly in root data.
* `fixed_function.hpp` — its role is replaced by boxed closures.

## Semantics

* Exhausted allocations return `None` instead of an empty aggregate with an invalid token, and
  `UploadQueue::new` returns `None` instead of producing an invalid queue.
* C++ move semantics become ordinary Rust moves; a destroyed or moved-from queue is not usable.
* `UploadQueue::upload_with_compute` takes a Rust closure receiving an `UploadFrame` (command buffer +
  staging range) instead of a template functor, and `set_submit_callback` takes a boxed closure
  instead of a C function pointer plus context.
* Panics replace the upstream asserts: delete-queue capacity overflow and decreasing retire values
  panic, upload preconditions (region size, pitches, staging capacity) panic, and allocator storage
  preconditions are `debug_assert`ed in [`BumpAllocator::new`] and [`HeapAllocator::new`].
* Operations whose raw-pointer arguments the C++ API treats as plain pointers stay `unsafe` here:
  `UploadQueue::upload_texture`, `UploadQueue::write_timestamp`, and `texture_upload::upload_texture`.
* Drop mirrors upstream: `DeleteQueue` does *not* run pending callbacks on destruction, because they
  may free resources the GPU still uses. Drain it after `wait_idle` and before `destroy_device`.
  `TextureAllocator::free` destroys the texture immediately.
* Allocators and queues are externally synchronized, exactly like the C++ types.

## Example

```rust
use no_graphics_api_sys as gpu;
use no_graphics_api_utility::{BumpAllocator, DeleteQueue};

unsafe {
    let mut init = gpu::DeviceInit::default();
    gpu::create_device(&gpu::DeviceDesc::default(), &mut init);
    assert_eq!(init.error, gpu::Error::none);

    let mut heap = gpu::GpuHeap::default();
    gpu::create_gpu_heap(init.device, 1 << 20, gpu::MemoryType::cpu_visible, &mut heap);

    let mut allocator = BumpAllocator::new(heap.range);
    let staging = allocator.allocate(1024).expect("bump range");

    let semaphore = gpu::create_timeline_semaphore(init.device, 0);
    let mut delete_queue = DeleteQueue::new(semaphore, 16);
    delete_queue.defer(1, move || {
        // Runs on the thread that ticks the queue once the timeline reaches 1.
        let _ = staging;
    });

    gpu::wait_idle(init.device);
    delete_queue.drain();
    gpu::destroy_gpu_heap(&heap);
    gpu::destroy_timeline_semaphore(semaphore);
    gpu::destroy_device(init.device);
}
```

## Testing

```sh
cargo test -p no-graphics-api-utility
```

* `tests/layout.rs` pins the shader-type sizes, alignments, and matrix row strides.
* `tests/allocators.rs` is the port of upstream `utility/tests/allocator_test.cpp`: address domains,
  token reuse, typed allocations, a 20 000-iteration fragmentation run with overlap checks, and an
  8-thread concurrent bump allocation test.
* `tests/gpu_objects.rs` exercises timeline retirement and texture placement on a headless device and
  skips when no Vulkan device is available.
* `tests/upload_queue.rs` is the port of upstream `utility/tests/upload_queue_test.cpp`: ring wrapping,
  tiled sub-region texture uploads, pitched copies, attachment consumption, reclaim before flush,
  timestamp batches, destroy-time flush, move semantics, the operation limit, submission handoff,
  non-power-of-two rings with 1–3 pending batches, gated batch limits, and cross-queue uploads (the
  multi-queue scenarios skip on single-queue devices), plus a volume upload that splits along slices.
  Every uploaded byte is verified through a guarded readback heap. The compute-dispatch scenario needs a precompiled shader and is reduced to a
  barrier/plumbing check.
