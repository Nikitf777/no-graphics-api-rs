//! Port of upstream `utility/tests/allocator_test.cpp` (allocator portions).

#![allow(clippy::useless_conversion)] // `size_t` is `u64` on this target; see the crate docs.

use core::ptr::null_mut;

use no_graphics_api_sys::GpuCpuRange;
use no_graphics_api_utility::{BumpAllocator, HeapAllocation, HeapAllocator, PlacedTexture};

#[repr(align(64))]
struct Storage<const N: usize>([u8; N]);

impl<const N: usize> Storage<N> {
    fn new() -> Self {
        Self([0; N])
    }

    fn range(&mut self) -> GpuCpuRange<u8> {
        GpuCpuRange::new(self.0.as_mut_ptr(), null_mut(), self.0.len())
    }
}

fn pointer_range(cpu: &mut [u8], gpu: &mut [u8]) -> GpuCpuRange<u8> {
    GpuCpuRange::new(cpu.as_mut_ptr(), gpu.as_mut_ptr(), cpu.len())
}

fn plus<T>(pointer: *mut T, offset: usize) -> *mut T {
    pointer.wrapping_add(offset)
}

#[test]
fn defaults() {
    let placed = PlacedTexture::default();
    assert!(placed.texture.is_null());
    assert_eq!(placed.token, 0);
    assert_eq!(HeapAllocator::ALIGNMENT, 16);
    assert_eq!(BumpAllocator::ALIGNMENT, 16);
    assert_eq!(HeapAllocator::MAXIMUM_SIZE, u64::from(u32::MAX) * 16);
}

#[test]
fn heap_allocator_basics() {
    let mut cpu = Storage::<4096>::new();
    let mut gpu = Storage::<4096>::new();
    let storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let mut allocator = unsafe { HeapAllocator::new(storage, 8) };

    let first = allocator.allocate(1).expect("first allocation");
    let second = allocator.allocate(17).expect("second allocation");
    let third = allocator.allocate(32).expect("third allocation");

    assert_eq!(first.range.cpu, cpu.0.as_mut_ptr());
    assert_eq!(first.range.gpu, gpu.0.as_mut_ptr());
    assert_eq!(first.range.size, 1);
    assert_eq!(second.range.cpu, plus(cpu.0.as_mut_ptr(), 16));
    assert_eq!(second.range.gpu, plus(gpu.0.as_mut_ptr(), 16));
    assert_eq!(second.range.size, 17);
    assert_eq!(third.range.cpu, plus(cpu.0.as_mut_ptr(), 48));
    assert_eq!(third.range.gpu, plus(gpu.0.as_mut_ptr(), 48));
    assert_eq!(third.range.size, 32);
    assert_ne!(first.token, second.token);
    assert_ne!(first.token, third.token);
    assert_ne!(second.token, third.token);

    allocator.free(second);
    allocator.free(first);
    allocator.free(third);

    let whole = allocator.allocate(4096).expect("coalesced allocation");
    assert_eq!(whole.range.cpu, cpu.0.as_mut_ptr());
    assert_eq!(whole.range.gpu, gpu.0.as_mut_ptr());
    assert_eq!(whole.range.size, 4096);
    assert!(allocator.allocate(16).is_none());
    allocator.free(whole);

    allocator.reset();
    assert!(allocator.allocate(4097).is_none());

    let mut partial = Storage::<17>::new();
    let partial_storage = partial.range();
    let mut partial_allocator = unsafe { HeapAllocator::new(partial_storage, 2) };
    assert_eq!(
        partial_allocator
            .allocate(1)
            .expect("partial allocation")
            .range
            .cpu,
        partial.0.as_mut_ptr()
    );
    assert!(partial_allocator.allocate(1).is_none());
}

#[test]
fn heap_allocator_address_domains() {
    let mut cpu = Storage::<128>::new();
    let mut gpu = Storage::<128>::new();

    let cpu_only_storage = GpuCpuRange::new(cpu.0.as_mut_ptr(), null_mut(), 128);
    let mut cpu_only = unsafe { HeapAllocator::new(cpu_only_storage, 1) };
    let allocation = cpu_only.allocate(1).expect("cpu-only allocation");
    assert_eq!(allocation.range.cpu, cpu.0.as_mut_ptr());
    assert!(allocation.range.gpu.is_null());
    assert_eq!(allocation.range.size, 1);
    cpu_only.free(allocation);

    let gpu_only_storage = GpuCpuRange::new(null_mut(), gpu.0.as_mut_ptr(), 128);
    let mut gpu_only = unsafe { HeapAllocator::new(gpu_only_storage, 1) };
    let allocation = gpu_only.allocate(1).expect("gpu-only allocation");
    assert!(allocation.range.cpu.is_null());
    assert_eq!(allocation.range.gpu, gpu.0.as_mut_ptr());
    assert_eq!(allocation.range.size, 1);
    gpu_only.free(allocation);
}

#[test]
fn heap_allocator_limit_and_token_reuse() {
    let mut cpu = Storage::<128>::new();
    let mut gpu = Storage::<128>::new();
    let storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let mut allocator = unsafe { HeapAllocator::new(storage, 2) };

    let first = allocator.allocate(16).expect("first allocation");
    let second = allocator.allocate(16).expect("second allocation");
    assert!(allocator.allocate(16).is_none());
    assert_eq!(allocator.allocation_count(), 2);

    allocator.free(first);
    let replacement = allocator.allocate(16).expect("reused allocation");
    assert_eq!(replacement.range.gpu, first.range.gpu);
    assert_eq!(replacement.token, first.token);

    allocator.free(replacement);
    allocator.free(second);
    assert_eq!(allocator.allocation_count(), 0);
    assert_eq!(
        allocator.allocate(128).expect("whole range").range.gpu,
        gpu.0.as_mut_ptr()
    );

    allocator.reset();
    let after_reset = allocator.allocate(16).expect("after reset");
    allocator.free(after_reset);
    assert_eq!(allocator.allocation_count(), 0);
    assert!(allocator.allocate(129).is_none());
}

#[test]
fn typed_allocations() {
    let mut cpu = Storage::<64>::new();
    let mut gpu = Storage::<64>::new();
    let storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let mut allocator = unsafe { HeapAllocator::new(storage, 1) };

    let allocation: HeapAllocation<u32> = allocator
        .allocate_typed::<u32>(3)
        .expect("typed heap allocation");
    assert_eq!(allocation.range.cpu, cpu.0.as_mut_ptr().cast::<u32>());
    assert_eq!(allocation.range.gpu, gpu.0.as_mut_ptr().cast::<u32>());
    assert_eq!(allocation.range.size, 12);
    allocator.free(allocation);

    let bump_storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let mut bump = unsafe { BumpAllocator::new(bump_storage) };
    let allocation: GpuCpuRange<u16> = bump
        .allocate_typed::<u16>(5)
        .expect("typed bump allocation");
    assert_eq!(allocation.cpu, cpu.0.as_mut_ptr().cast::<u16>());
    assert_eq!(allocation.gpu, gpu.0.as_mut_ptr().cast::<u16>());
    assert_eq!(allocation.size, 10);
}

#[test]
fn heap_allocator_fragmentation() {
    const ALLOCATION_COUNT: usize = 64;
    let mut gpu = Storage::<4096>::new();
    let storage = GpuCpuRange::new(null_mut(), gpu.0.as_mut_ptr(), 4096);
    let mut allocator = unsafe { HeapAllocator::new(storage, ALLOCATION_COUNT as u32) };
    let mut allocations: [Option<HeapAllocation<u8>>; ALLOCATION_COUNT] = [None; ALLOCATION_COUNT];
    let mut random: u32 = 0x1234_5678;

    for _ in 0..20_000 {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let slot = (random % ALLOCATION_COUNT as u32) as usize;
        if let Some(allocation) = allocations[slot].take() {
            allocator.free(allocation);
            continue;
        }

        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let size = u64::from(random % 97) + 1;
        let Some(allocation) = allocator.allocate(size) else {
            continue;
        };

        assert_eq!(allocation.range.size, size);
        let begin = allocation.range.gpu as usize;
        let storage_begin = gpu.0.as_mut_ptr() as usize;
        assert!(begin >= storage_begin);
        assert!(begin + size as usize <= storage_begin + 4096);
        for (other_slot, other) in allocations.iter().enumerate() {
            if other_slot == slot {
                continue;
            }
            if let Some(other) = other {
                assert!(!ranges_overlap(&allocation, other), "allocations overlap");
            }
        }

        allocations[slot] = Some(allocation);
    }

    for allocation in allocations.into_iter().flatten() {
        allocator.free(allocation);
    }
    assert_eq!(
        allocator.allocate(4096).expect("fully coalesced").range.gpu,
        gpu.0.as_mut_ptr()
    );
}

fn ranges_overlap(lhs: &HeapAllocation<u8>, rhs: &HeapAllocation<u8>) -> bool {
    let lhs_begin = lhs.range.gpu as usize;
    let rhs_begin = rhs.range.gpu as usize;
    let lhs_size = (u64::from(lhs.range.size) + 15) & !15u64;
    let rhs_size = (u64::from(rhs.range.size) + 15) & !15u64;
    lhs_begin < rhs_begin + rhs_size as usize && rhs_begin < lhs_begin + lhs_size as usize
}

#[test]
fn bump_allocator() {
    let mut cpu = Storage::<64>::new();
    let mut gpu = Storage::<64>::new();
    let storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let mut allocator = unsafe { BumpAllocator::new(storage) };

    let first = allocator.allocate(1).expect("first");
    let second = allocator.allocate(17).expect("second");
    let third = allocator.allocate(16).expect("third");
    assert_eq!(first.cpu, cpu.0.as_mut_ptr());
    assert_eq!(first.gpu, gpu.0.as_mut_ptr());
    assert_eq!(first.size, 1);
    assert_eq!(second.cpu, plus(cpu.0.as_mut_ptr(), 16));
    assert_eq!(second.size, 17);
    assert_eq!(third.cpu, plus(cpu.0.as_mut_ptr(), 48));
    assert_eq!(third.size, 16);
    assert!(allocator.allocate(1).is_none());

    allocator.reset();
    assert_eq!(
        allocator.allocate(64).expect("after reset").gpu,
        gpu.0.as_mut_ptr()
    );

    let mut partial = Storage::<17>::new();
    let partial_storage = partial.range();
    let mut partial_allocator = unsafe { BumpAllocator::new(partial_storage) };
    assert_eq!(
        partial_allocator.allocate(1).expect("first").cpu,
        partial.0.as_mut_ptr()
    );
    assert_eq!(
        partial_allocator.allocate(1).expect("second").cpu,
        plus(partial.0.as_mut_ptr(), 16)
    );
    assert!(partial_allocator.allocate(1).is_none());
}

#[test]
fn atomic_bump_allocator() {
    const THREAD_COUNT: usize = 8;
    const ALLOCATIONS_PER_THREAD: usize = 64;
    const ALLOCATION_COUNT: usize = THREAD_COUNT * ALLOCATIONS_PER_THREAD;
    const CAPACITY: usize = ALLOCATION_COUNT * 16;

    let mut cpu = Storage::<CAPACITY>::new();
    let mut gpu = Storage::<CAPACITY>::new();
    let storage = pointer_range(&mut cpu.0, &mut gpu.0);
    let allocator = unsafe { BumpAllocator::new(storage) };

    let cpu_base = cpu.0.as_mut_ptr() as usize;
    let gpu_base = gpu.0.as_mut_ptr() as usize;
    let results: Vec<Vec<usize>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREAD_COUNT)
            .map(|_| {
                scope.spawn(|| {
                    (0..ALLOCATIONS_PER_THREAD)
                        .map(|_| {
                            let allocation =
                                allocator.allocate_atomic(1).expect("concurrent allocation");
                            let cpu_offset = allocation.cpu as usize - cpu_base;
                            assert_eq!(allocation.gpu as usize - gpu_base, cpu_offset);
                            assert_eq!(allocation.size, 1);
                            assert_eq!(cpu_offset % BumpAllocator::ALIGNMENT as usize, 0);
                            assert!(cpu_offset < CAPACITY);
                            cpu_offset
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });

    let mut offsets: Vec<usize> = results.into_iter().flatten().collect();
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), ALLOCATION_COUNT);

    assert!(allocator.allocate_atomic(1).is_none());
    let mut allocator = allocator;
    allocator.reset();
    assert_eq!(
        allocator
            .allocate_atomic(CAPACITY as u64)
            .expect("after reset")
            .cpu,
        cpu.0.as_mut_ptr()
    );
}

#[test]
fn maximum_heap_range() {
    let maximum_size = HeapAllocator::MAXIMUM_SIZE;
    let storage = GpuCpuRange::new(null_mut(), 16 as *mut u8, maximum_size as usize);
    let mut allocator = unsafe { HeapAllocator::new(storage, 1) };

    let allocation = allocator
        .allocate(maximum_size)
        .expect("maximum allocation");
    assert_eq!(allocation.range.gpu, 16 as *mut u8);
    assert_eq!(u64::from(allocation.range.size), maximum_size);
    allocator.free(allocation);
}
