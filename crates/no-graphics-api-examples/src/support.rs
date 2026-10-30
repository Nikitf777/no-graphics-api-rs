//! Window, device, and shader helpers shared by the example applications.

use core::ffi::{c_char, c_void};
use core::ptr::null_mut;

use no_graphics_api_sys as sys;
use winit::event_loop::EventLoop;
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::Window;

/// 4-byte-aligned SPIR-V storage: `include_bytes!` produces alignment-1 data, but the API requires
/// `uint32`-aligned shader binaries.
#[repr(C, align(4))]
pub struct AlignedShader<const N: usize>([u8; N]);

impl<const N: usize> AlignedShader<N> {
    pub const fn from_bytes(bytes: &[u8; N]) -> Self {
        Self(*bytes)
    }

    pub const fn bytes(&self) -> &[u8] {
        &self.0
    }
}

unsafe extern "C" fn error_callback(message: *const c_char, fatal: bool) {
    let text = unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy();
    eprintln!("NoGraphicsAPI error (fatal: {fatal}): {text}");
}

/// Creates the event loop, restricted to Wayland on Linux: the local presentation patch has no X11
/// surface support.
pub fn event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    #[allow(unused_mut)]
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        builder.with_wayland();
    }
    builder.build()
}

/// Returns the native handles the API expects: (`window`, `display`).
///
/// `window` is an `HWND` on Windows and a `wl_surface*` on Wayland; `display` is a `wl_display*` on
/// Wayland and unused elsewhere.
fn window_handles(window: &Window) -> Option<(*mut c_void, *mut c_void)> {
    let window_handle = window.window_handle().ok()?;
    let display_handle = window.display_handle().ok()?;

    match (window_handle.as_raw(), display_handle.as_raw()) {
        (RawWindowHandle::Wayland(surface), RawDisplayHandle::Wayland(display)) => {
            Some((surface.surface.as_ptr(), display.display.as_ptr()))
        }
        (RawWindowHandle::Win32(handle), _) => Some((handle.hwnd.get() as *mut c_void, null_mut())),
        _ => {
            eprintln!("unsupported window handle: this example needs Wayland or Win32");
            None
        }
    }
}

/// Creates a swapchain-backed device for `window`, sized to its current drawable.
pub fn create_device(window: &Window, swapchain_format: sys::Format) -> Option<*mut sys::Device> {
    unsafe { sys::set_error_callback(Some(error_callback)) };

    let (native_window, native_display) = window_handles(window)?;
    let size = window.inner_size();
    let description = sys::DeviceDesc {
        window: native_window,
        display: native_display,
        swapchain_format,
        swapchain_extent: sys::uint32x2::new(size.width.max(1), size.height.max(1)),
        ..sys::DeviceDesc::default()
    };

    let mut init = sys::DeviceInit::default();
    unsafe { sys::create_device(&description, &mut init) };
    if init.device.is_null() {
        eprintln!("device creation failed: {:?}", init.error);
        return None;
    }

    Some(init.device)
}

/// Handles a window resize: the swapchain follows the new drawable size on the next acquire.
///
/// # Safety
/// `device` must be the live device created for the window.
pub unsafe fn set_extent(device: *mut sys::Device, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }

    unsafe { sys::set_swapchain_extent(device, sys::uint32x2::new(width, height)) };
}

/// Optional frame limit for smoke runs (`NGA_EXAMPLE_FRAMES`), so examples can exit on their own.
pub fn frame_limit() -> Option<u64> {
    std::env::var("NGA_EXAMPLE_FRAMES").ok()?.parse().ok()
}

pub fn shader_stage(code: &'static [u8], entry_point: &'static std::ffi::CStr) -> sys::ShaderStage {
    sys::ShaderStage {
        code: sys::ByteSpan::from_slice(code),
        entry_point: entry_point.as_ptr(),
        ..sys::ShaderStage::default()
    }
}
