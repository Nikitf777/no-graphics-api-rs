//! Rust port of upstream `NoGraphicsAPIUtility/bump_allocator.hpp`.
//!
//! A 16-byte-aligned bump pointer. Reservations are rounded up to [`BumpAllocator::ALIGNMENT`] while
//! the returned range keeps the requested byte size.

use core::ptr::null_mut;
use core::sync::atomic::{AtomicU64, Ordering};

use no_graphics_api_sys::GpuCpuRange;

fn offset_pointer<T>(pointer: *mut T, offset: u64) -> *mut T {
    if pointer.is_null() {
        return null_mut();
    }

    (pointer as usize).wrapping_add(offset as usize) as *mut T
}

/// Linear allocator over one GPU heap's CPU/GPU address pair.
pub struct BumpAllocator {
    storage: GpuCpuRange<u8>,
    offset: AtomicU64,
}

// The offset is atomic, `allocate_atomic` is the only `&self` operation, and every other operation
// takes `&mut self`. The storage itself is plain pointer data, so sharing the allocator is safe.
unsafe impl Send for BumpAllocator {}
unsafe impl Sync for BumpAllocator {}

impl BumpAllocator {
    /// Reservation granularity.
    pub const ALIGNMENT: u64 = 16;

    /// # Safety
    /// `storage` must outlive the allocator, be nonempty, expose at least one non-null CPU or GPU
    /// address, and align every exposed address to [`Self::ALIGNMENT`].
    pub unsafe fn new(storage: GpuCpuRange<u8>) -> Self {
        debug_assert!(u64::from(storage.size) != 0);
        debug_assert!(!storage.cpu.is_null() || !storage.gpu.is_null());
        debug_assert!(
            storage.cpu.is_null() || (storage.cpu as usize) % Self::ALIGNMENT as usize == 0
        );
        debug_assert!(
            storage.gpu.is_null() || (storage.gpu as usize) % Self::ALIGNMENT as usize == 0
        );

        Self {
            storage,
            offset: AtomicU64::new(0),
        }
    }

    /// Reserves `byte_size` bytes, or returns `None` when the storage is exhausted.
    ///
    /// A zero-size request is a programming error.
    pub fn allocate(&mut self, byte_size: u64) -> Option<GpuCpuRange<u8>> {
        debug_assert!(byte_size != 0);
        let storage_size = u64::from(self.storage.size);
        let offset = self.offset.load(Ordering::Relaxed);
        if byte_size > storage_size - offset {
            return None;
        }

        let allocation = GpuCpuRange::new(
            offset_pointer(self.storage.cpu, offset),
            offset_pointer(self.storage.gpu, offset),
            byte_size as usize,
        );
        let remaining = storage_size - offset;
        let aligned_size = (byte_size + Self::ALIGNMENT - 1) & !(Self::ALIGNMENT - 1);
        self.offset.store(
            offset
                + if aligned_size < remaining {
                    aligned_size
                } else {
                    remaining
                },
            Ordering::Relaxed,
        );
        Some(allocation)
    }

    /// Lock-free variant of [`Self::allocate`]; concurrent calls return disjoint ranges.
    ///
    /// A zero-size request is a programming error.
    pub fn allocate_atomic(&self, byte_size: u64) -> Option<GpuCpuRange<u8>> {
        debug_assert!(byte_size != 0);
        let storage_size = u64::from(self.storage.size);
        let aligned_size = (byte_size + Self::ALIGNMENT - 1) & !(Self::ALIGNMENT - 1);
        let mut offset = self.offset.load(Ordering::Relaxed);
        loop {
            if byte_size > storage_size - offset {
                return None;
            }

            let remaining = storage_size - offset;
            let next_offset = offset
                + if aligned_size < remaining {
                    aligned_size
                } else {
                    remaining
                };
            match self.offset.compare_exchange_weak(
                offset,
                next_offset,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(GpuCpuRange::new(
                        offset_pointer(self.storage.cpu, offset),
                        offset_pointer(self.storage.gpu, offset),
                        byte_size as usize,
                    ));
                }
                Err(previous) => offset = previous,
            }
        }
    }

    /// Typed variant of [`Self::allocate`]; the range size is `element_count * size_of::<T>()`.
    pub fn allocate_typed<T>(&mut self, element_count: u64) -> Option<GpuCpuRange<T>> {
        assert!(core::mem::align_of::<T>() as u64 <= Self::ALIGNMENT);
        let byte_size = element_count * core::mem::size_of::<T>() as u64;
        let allocation = self.allocate(byte_size)?;
        Some(GpuCpuRange::new(
            allocation.cpu.cast(),
            allocation.gpu.cast(),
            byte_size as usize,
        ))
    }

    /// Typed variant of [`Self::allocate_atomic`].
    pub fn allocate_atomic_typed<T>(&self, element_count: u64) -> Option<GpuCpuRange<T>> {
        assert!(core::mem::align_of::<T>() as u64 <= Self::ALIGNMENT);
        let byte_size = element_count * core::mem::size_of::<T>() as u64;
        let allocation = self.allocate_atomic(byte_size)?;
        Some(GpuCpuRange::new(
            allocation.cpu.cast(),
            allocation.gpu.cast(),
            byte_size as usize,
        ))
    }

    /// Invalidates every previous allocation.
    pub fn reset(&mut self) {
        self.offset.store(0, Ordering::Relaxed);
    }

    /// Bytes handed out so far, rounded up to the reservation granularity.
    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }
}
