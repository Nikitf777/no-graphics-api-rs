//! Low-level unsafe Rust bindings for [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI).
//!
//! NoGraphicsAPI is a thin, exception-free C++20 wrapper over a single fixed GPU feature set
//! (Vulkan on Windows/Linux, Metal 4 on Apple platforms). This crate mirrors that API one-to-one
//! as a raw C ABI: every function is `unsafe`, aggregates are passed by pointer, and no validation,
//! reference counting, or lifetime tracking is added.
//!
//! # Naming
//!
//! Types and enumerators keep their C++ names (`Device`, `Format::rgba8_unorm`, `Stage::fragment`).
//! Functions are exported under the C++ names as well (`create_device`, `begin_render_pass`, ...).
//! The bindgen output with its `nga_` prefix is still available in [`ffi`].
//!
//! # Build requirements
//!
//! * A C++20 compiler, `libclang` for bindgen, and the Vulkan headers (`vulkan/vulkan.h`).
//! * The upstream sources are vendored in `vendor/NoGraphicsAPI`. Set `NO_GRAPHICS_API_SOURCE`
//!   to compile against a different NoGraphicsAPI checkout.
//! * `VULKAN_SDK` is honored when set. On non-Apple targets the crate links the Vulkan loader
//!   (`vulkan`, or `vulkan-1` on Windows) and the loader must be installed at runtime.
//! * On Linux the default `wayland` feature enables the local Wayland presentation patch and needs
//!   `<wayland-client.h>`. Use `default-features = false` for headless-only builds.
//!
//! # Presentation
//!
//! Pass `DeviceDesc::window` to create a swapchain-backed device: an `HWND` on Windows, a
//! `CAMetalLayer*` on macOS, or a `wl_surface*` on Wayland together with `DeviceDesc::display`
//! (`wl_display*`) and `DeviceDesc::swapchain_extent` (Wayland surfaces report no fixed extent).
//! Call [`set_swapchain_extent`] on resize; a zero component keeps the current size. Wayland support
//! is a local patch over upstream; see `vendor/patches`.
//!
//! # Safety contract
//!
//! The C++ documentation is normative; the essentials:
//!
//! * Every resource handle is an opaque pointer. Destruction is immediate, so destroy a resource
//!   only when no recorded or executing frame uses it. Call [`wait_idle`] and drain the optional
//!   `NoGraphicsAPIUtility` delete queues before destroying the device, then destroy all command
//!   pools before their device.
//! * `Device` access is externally synchronized per `(device, queue_index)` and per command pool
//!   (including recording its buffers). Different queues and pools may run concurrently.
//!   Resource creation/destruction, immutable queries, and timeline waits may run concurrently.
//! * `submit` and `submit_and_present` need one ended command buffer subset per submission, a
//!   completion [`TimelinePoint`], and a non-empty acquire submitted on queue zero with
//!   `submit_and_present`.
//! * Span fields (`SubmitDesc::commands`, `GraphicsPSODesc::color_targets`, ...) are borrowed for
//!   the duration of the call only. Use [`Span::from_slice`] or [`Span::new`].
//! * The root data pointer passed to `draw`, `dispatch`, ... is a 16-byte-aligned GPU address owned
//!   by the application; it must stay alive and unchanged until the GPU is done with it.
//! * The window/layer in [`DeviceDesc::window`] must outlive the device.
//!
//! Initialization and creation failures return an [`Error`], a null pointer, or an empty aggregate;
//! details go to the optional error callback installed with [`set_error_callback`].

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
#![allow(clippy::missing_safety_doc, clippy::upper_case_acronyms)]

use core::marker::PhantomData;
use core::ptr::{null, null_mut};

/// Raw bindgen output: the C ABI shim (`csi/shim.h`) translated from the C++ headers.
///
/// Prefer the re-exports at the crate root; this module is the unedited bindgen result and exists
/// for code that wants the original `nga_`-prefixed symbols or the C++ namespace layout.
#[allow(clippy::all, dead_code, improper_ctypes, unused_imports)]
pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

    /// Flattened access to the C ABI: `ffi::nga_create_device`, `ffi::gpu::DeviceDesc`.
    pub use root::*;
}

pub use ffi::root::gpu::*;

pub use ffi::root::{
    nga_acquire as acquire, nga_barrier as barrier, nga_begin_commands as begin_commands,
    nga_begin_render_pass as begin_render_pass, nga_bind_pso as bind_pso,
    nga_copy_memory as copy_memory, nga_copy_memory_to_texture as copy_memory_to_texture,
    nga_copy_sampler_descriptors as copy_sampler_descriptors,
    nga_copy_texture_descriptors as copy_texture_descriptors,
    nga_copy_texture_to_memory as copy_texture_to_memory,
    nga_create_command_pool as create_command_pool, nga_create_compute_pso as create_compute_pso,
    nga_create_device as create_device, nga_create_gpu_heap as create_gpu_heap,
    nga_create_graphics_pso as create_graphics_pso, nga_create_mesh_pso as create_mesh_pso,
    nga_create_render_view as create_render_view,
    nga_create_sampler_descriptor_heap as create_sampler_descriptor_heap,
    nga_create_texture as create_texture,
    nga_create_texture_descriptor_heap as create_texture_descriptor_heap,
    nga_create_texture_heap as create_texture_heap,
    nga_create_timeline_semaphore as create_timeline_semaphore,
    nga_destroy_command_pool as destroy_command_pool, nga_destroy_device as destroy_device,
    nga_destroy_gpu_heap as destroy_gpu_heap, nga_destroy_pso as destroy_pso,
    nga_destroy_render_view as destroy_render_view,
    nga_destroy_sampler_descriptor_heap as destroy_sampler_descriptor_heap,
    nga_destroy_texture as destroy_texture,
    nga_destroy_texture_descriptor_heap as destroy_texture_descriptor_heap,
    nga_destroy_texture_heap as destroy_texture_heap,
    nga_destroy_timeline_semaphore as destroy_timeline_semaphore, nga_dispatch as dispatch,
    nga_dispatch_indirect as dispatch_indirect, nga_draw as draw, nga_draw_indexed as draw_indexed,
    nga_draw_indexed_indirect as draw_indexed_indirect, nga_draw_indirect as draw_indirect,
    nga_draw_meshlets as draw_meshlets, nga_draw_meshlets_indirect as draw_meshlets_indirect,
    nga_end_commands as end_commands, nga_end_render_pass as end_render_pass,
    nga_get_device_caps as get_device_caps, nga_get_drawable_extent as get_drawable_extent,
    nga_get_texture_format_info as get_texture_format_info,
    nga_get_texture_size_align as get_texture_size_align,
    nga_gpu_range_of_cpu_range as gpu_range_of_cpu_range,
    nga_gpu_range_of_heap as gpu_range_of_heap, nga_initialize_texture as initialize_texture,
    nga_read_timestamps as read_timestamps, nga_reset_command_pool as reset_command_pool,
    nga_set_depth_stencil as set_depth_stencil, nga_set_error_callback as set_error_callback,
    nga_set_sampler_descriptor_heap as set_sampler_descriptor_heap, nga_set_scissor as set_scissor,
    nga_set_swapchain_extent as set_swapchain_extent,
    nga_set_swapchain_format as set_swapchain_format,
    nga_set_texture_descriptor_heap as set_texture_descriptor_heap,
    nga_set_viewport as set_viewport, nga_submit as submit,
    nga_submit_and_present as submit_and_present,
    nga_supports_texture_format as supports_texture_format,
    nga_timeline_completed_value as timeline_completed_value, nga_wait_idle as wait_idle,
    nga_wait_timeline as wait_timeline, nga_write_sampler_descriptor as write_sampler_descriptor,
    nga_write_texture_descriptor as write_texture_descriptor,
    nga_write_timestamp as write_timestamp,
};

mod defaults;

impl<T> Span<T> {
    /// An empty span (`nullptr`, zero elements).
    pub const fn empty() -> Self {
        Self {
            _phantom_0: PhantomData,
            data: null_mut(),
            size: 0,
        }
    }

    /// A span over `size` elements starting at `data`.
    pub const fn new(data: *mut T, size: usize) -> Self {
        Self {
            _phantom_0: PhantomData,
            data,
            size: size as _,
        }
    }

    /// A span over `data`, valid only while the borrow lives.
    pub fn from_slice(data: &mut [T]) -> Self {
        Self::new(data.as_mut_ptr(), data.len())
    }

    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// The elements behind this span.
    ///
    /// # Safety
    /// `data` must be valid for `size` initialized elements for the returned lifetime.
    pub unsafe fn as_slice<'a>(&self) -> &'a [T] {
        if self.size == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.data, self.size as usize) }
        }
    }
}

impl<T> Default for Span<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl ByteSpan {
    pub const fn empty() -> Self {
        Self {
            data: null(),
            size: 0,
        }
    }

    pub const fn new(data: *const u8, size: usize) -> Self {
        Self {
            data,
            size: size as _,
        }
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        Self::new(bytes.as_ptr(), bytes.len())
    }

    /// Views a single value as bytes, matching the C++ `ByteSpan(const T&)` constructor.
    pub fn from_value<T>(value: &T) -> Self {
        Self::new((value as *const T).cast(), core::mem::size_of::<T>())
    }

    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// # Safety
    /// `data` must be valid for `size` bytes for the returned lifetime.
    pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
        if self.size == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.data, self.size as usize) }
        }
    }
}

impl Default for ByteSpan {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> GpuCpuRange<T> {
    pub const fn empty() -> Self {
        Self {
            _phantom_0: PhantomData,
            cpu: null_mut(),
            gpu: null_mut(),
            size: 0,
        }
    }

    /// `size` is a byte count, independent of `T`.
    pub const fn new(cpu: *mut T, gpu: *mut T, size: usize) -> Self {
        Self {
            _phantom_0: PhantomData,
            cpu,
            gpu,
            size: size as _,
        }
    }
}

impl<T> Default for GpuCpuRange<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl uint32x2 {
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

impl uint32x3 {
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }
}

impl DeviceCaps {
    /// The adapter name, or `None` when the driver reported no name.
    ///
    /// # Safety
    /// The device must still be alive; the string is owned by the driver.
    pub unsafe fn device_name_cstr(&self) -> Option<&core::ffi::CStr> {
        unsafe {
            self.device_name
                .as_ref()
                .map(|name| core::ffi::CStr::from_ptr(name))
        }
    }
}

impl TextureUsage {
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl Stage {
    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl Access {
    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl RenderingFlags {
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
