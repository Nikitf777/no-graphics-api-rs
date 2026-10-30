//! Device-backed utility behavior: delete queue retirement and texture placement.
//!
//! Skips when no Vulkan device is available.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::rc::Rc;

use no_graphics_api_sys as gpu;
use no_graphics_api_utility::{DeleteQueue, TextureAllocator};

unsafe fn create_headless_device() -> Option<*mut gpu::Device> {
    let mut init = gpu::DeviceInit::default();
    unsafe { gpu::create_device(&gpu::DeviceDesc::default(), &mut init) };
    if init.device.is_null() {
        eprintln!("no GPU available, skipping (error: {:?})", init.error);
        return None;
    }

    Some(init.device)
}

unsafe fn submit_empty_frame(
    device: *mut gpu::Device,
    pool: *mut gpu::CommandPool,
    semaphore: *mut gpu::TimelineSemaphore,
    value: u64,
) {
    unsafe {
        let commands = gpu::begin_commands(pool);
        gpu::end_commands(commands);
        let mut commands = [commands];
        let completion = gpu::TimelinePoint { semaphore, value };
        let desc = gpu::SubmitDesc {
            commands: gpu::Span::from_slice(&mut commands),
            waits: gpu::Span::empty(),
            completion,
        };
        gpu::submit(device, &desc, 0);
        gpu::wait_timeline(completion);
    }
}

#[test]
fn delete_queue_ticks_and_drains() {
    unsafe {
        let Some(device) = create_headless_device() else {
            return;
        };
        let semaphore = gpu::create_timeline_semaphore(device, 0);
        assert!(!semaphore.is_null());
        let pool = gpu::create_command_pool(device, 0);
        assert!(!pool.is_null());

        let counter = Rc::new(Cell::new(0u32));
        let mut queue = DeleteQueue::new(semaphore, 4);
        for retire_value in [1u64, 1, 2] {
            let counter = Rc::clone(&counter);
            queue.defer(retire_value, move || counter.set(counter.get() + 1));
        }
        assert_eq!(queue.len(), 3);

        queue.tick();
        assert_eq!(
            counter.get(),
            0,
            "nothing retires before the timeline advances"
        );

        submit_empty_frame(device, pool, semaphore, 1);
        queue.tick();
        assert_eq!(
            counter.get(),
            2,
            "retire values at or below the completed value run"
        );
        assert_eq!(queue.len(), 1);

        queue.drain();
        assert_eq!(counter.get(), 3);
        assert!(queue.is_empty());

        drop(queue);
        gpu::destroy_command_pool(pool);
        gpu::destroy_timeline_semaphore(semaphore);
        gpu::destroy_device(device);
    }
}

#[test]
fn delete_queue_rejects_capacity_overflow() {
    // The queue never reads the timeline here, so a dangling pointer is enough.
    let mut queue =
        unsafe { DeleteQueue::new(NonNull::<gpu::TimelineSemaphore>::dangling().as_ptr(), 1) };
    queue.defer(1, || {});

    let overflow = catch_unwind(AssertUnwindSafe(|| queue.defer(1, || {})));
    assert!(overflow.is_err(), "deferring beyond capacity must panic");
    queue.drain();
}

#[test]
fn delete_queue_rejects_decreasing_retire_values() {
    let mut queue =
        unsafe { DeleteQueue::new(NonNull::<gpu::TimelineSemaphore>::dangling().as_ptr(), 4) };
    queue.defer(2, || {});

    let decreasing = catch_unwind(AssertUnwindSafe(|| queue.defer(1, || {})));
    assert!(decreasing.is_err(), "decreasing retire values must panic");
    queue.drain();
}

#[test]
fn texture_allocator_places_and_reuses() {
    unsafe {
        let Some(device) = create_headless_device() else {
            return;
        };
        let mut heap = gpu::TextureHeap::default();
        gpu::create_texture_heap(device, 1 << 20, &mut heap);
        assert!(!heap.owner.is_null(), "texture heap creation failed");

        let mut allocator = TextureAllocator::new(device, heap, 4);
        let desc = gpu::TextureDesc::default();

        let first = allocator.allocate(&desc).expect("first texture");
        let second = allocator.allocate(&desc).expect("second texture");
        assert!(!first.texture.is_null() && !second.texture.is_null());
        assert_ne!(first.token, second.token);

        allocator.free(first);
        let reused = allocator.allocate(&desc).expect("reused range");
        assert_eq!(
            reused.token, first.token,
            "freeing a range makes its token reusable"
        );

        let third = allocator.allocate(&desc).expect("third texture");
        let fourth = allocator.allocate(&desc).expect("fourth texture");
        assert!(
            allocator.allocate(&desc).is_none(),
            "max_textures limits placement"
        );

        let oversized = gpu::TextureDesc {
            extent: gpu::uint32x3::new(4096, 4096, 1),
            usage: gpu::TextureUsage::color_attachment,
            ..gpu::TextureDesc::default()
        };
        assert!(
            allocator.allocate(&oversized).is_none(),
            "oversized texture must not fit"
        );

        allocator.free(second);
        allocator.free(reused);
        allocator.free(third);
        allocator.free(fourth);

        drop(allocator);
        gpu::destroy_texture_heap(&heap);
        gpu::destroy_device(device);
    }
}
