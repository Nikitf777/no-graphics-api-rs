//! Rust port of upstream `NoGraphicsAPIUtility/texture_allocator.hpp`.
//!
//! Suballocates a [`TextureHeap`] with the shared [`crate::HeapAllocator`] range allocator and places
//! textures into it. Textures are created uninitialized; record `initialize_texture` before use.

use no_graphics_api_sys::{
    Device, SizeAlign, Texture, TextureDesc, TextureHeap, create_texture, destroy_texture,
    get_device_caps, get_texture_size_align,
};

use crate::heap_allocator::RangeAllocator;

/// A texture placed inside a [`TextureHeap`].
///
/// `token` identifies the heap range and must be freed through the allocator that produced it.
#[derive(Debug, Clone, Copy)]
pub struct PlacedTexture {
    pub texture: *mut Texture,
    pub token: u32,
}

impl Default for PlacedTexture {
    fn default() -> Self {
        Self {
            texture: core::ptr::null_mut(),
            token: 0,
        }
    }
}

impl PlacedTexture {
    pub fn is_null(&self) -> bool {
        self.texture.is_null()
    }
}

/// Places textures into one [`TextureHeap`].
///
/// Externally synchronize the allocator, like the C++ type. Free a placed texture once, with the
/// same allocator, after every view and GPU use has finished.
pub struct TextureAllocator {
    device: *mut Device,
    heap: TextureHeap,
    ranges: RangeAllocator,
}

impl TextureAllocator {
    /// # Safety
    /// `device` must be a live device, `heap` must be a live texture heap owned by that device, and
    /// both must outlive the allocator. `max_textures` must be in `1..=HeapAllocator::maximum_allocation_count`.
    pub unsafe fn new(device: *mut Device, heap: TextureHeap, max_textures: u32) -> Self {
        debug_assert!(!heap.owner.is_null());
        let caps = unsafe { &*get_device_caps(device) };
        Self {
            device,
            heap,
            ranges: RangeAllocator::new(
                u64::from(heap.size),
                max_textures,
                caps.texture_heap_alignment,
            ),
        }
    }

    /// Creates an uninitialized texture in the heap, or returns `None` when the heap is exhausted or
    /// creation failed.
    pub fn allocate(&mut self, desc: &TextureDesc) -> Option<PlacedTexture> {
        let mut requirements = SizeAlign::default();
        unsafe { get_texture_size_align(self.device, desc, &mut requirements) };
        if requirements.size == 0 {
            return None;
        }

        let range = self.ranges.allocate(u64::from(requirements.size))?;
        let texture = unsafe {
            create_texture(
                self.device,
                desc,
                &self.heap,
                u64::from(range.offset) * self.ranges.element_size(),
            )
        };
        if texture.is_null() {
            self.ranges.free(range.token);
            return None;
        }

        Some(PlacedTexture {
            texture,
            token: range.token,
        })
    }

    /// Destroys the texture and releases its heap range.
    pub fn free(&mut self, placed: PlacedTexture) {
        if placed.texture.is_null() {
            return;
        }

        self.ranges.free(placed.token);
        unsafe { destroy_texture(placed.texture) };
    }

    /// The device this allocator places textures on.
    pub fn device(&self) -> *mut Device {
        self.device
    }
}
