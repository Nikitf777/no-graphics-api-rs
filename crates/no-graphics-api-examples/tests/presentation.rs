//! Exercises the local Wayland presentation patch end to end.
//!
//! Starts a private headless weston with the GL renderer, creates a `wl_surface` through
//! libwayland-client, presents a few frames with `acquire`/`submit_and_present`, and checks the
//! swapchain-backed drawable extent. Skips when weston is unavailable.

#![cfg(target_os = "linux")]

use core::ffi::{c_char, c_int, c_void};
use core::ptr::{null, null_mut};
use std::ffi::CString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use no_graphics_api_examples::support::AlignedShader;
use no_graphics_api_sys as sys;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;
const SOCKET: &str = "nga-wayland-test";

static TRIANGLE_VERTEX: AlignedShader<{ include_bytes!("../shaders/triangle.vert.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/triangle.vert.spv"));
static TRIANGLE_FRAGMENT: AlignedShader<{ include_bytes!("../shaders/triangle.frag.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/triangle.frag.spv"));

// --- Minimal libwayland-client binding -----------------------------------

#[repr(C)]
struct WlDisplay {
    _private: [u8; 0],
}
#[repr(C)]
struct WlProxy {
    _private: [u8; 0],
}
#[repr(C)]
struct WlSurface {
    _private: [u8; 0],
}
#[repr(C)]
struct WlRegistry {
    _private: [u8; 0],
}
#[repr(C)]
struct WlInterface {
    name: *const c_char,
    version: c_int,
    method_count: c_int,
    methods: *const c_void,
    event_count: c_int,
    events: *const c_void,
}

#[repr(C)]
struct WlRegistryListener {
    global: unsafe extern "C" fn(*mut c_void, *mut WlRegistry, u32, *const c_char, u32),
    global_remove: unsafe extern "C" fn(*mut c_void, *mut WlRegistry, u32),
}

#[link(name = "wayland-client")]
unsafe extern "C" {
    fn wl_display_connect(name: *const c_char) -> *mut WlDisplay;
    fn wl_display_disconnect(display: *mut WlDisplay);
    fn wl_display_roundtrip(display: *mut WlDisplay) -> c_int;
    fn wl_display_flush(display: *mut WlDisplay) -> c_int;
    fn wl_proxy_add_listener(
        proxy: *mut WlProxy,
        implementation: *mut *const c_void,
        data: *mut c_void,
    ) -> c_int;
    fn wl_proxy_get_version(proxy: *mut WlProxy) -> u32;
    fn wl_proxy_marshal_flags(
        proxy: *mut WlProxy,
        opcode: u32,
        interface: *const WlInterface,
        version: u32,
        flags: u32,
        ...
    ) -> *mut WlProxy;

    static wl_registry_interface: WlInterface;
    static wl_compositor_interface: WlInterface;
    static wl_surface_interface: WlInterface;
}

unsafe extern "C" fn registry_global(
    data: *mut c_void,
    registry: *mut WlRegistry,
    name: u32,
    interface: *const c_char,
    _version: u32,
) {
    let name_ptr = unsafe { std::ffi::CStr::from_ptr(interface) };
    if name_ptr.to_bytes() != b"wl_compositor" {
        return;
    }

    // wl_registry::bind (opcode 0): name, interface name, version, new_id.
    let compositor = unsafe {
        wl_proxy_marshal_flags(
            registry.cast(),
            0,
            &wl_compositor_interface,
            wl_proxy_get_version(registry.cast()),
            0,
            name,
            wl_compositor_interface.name,
            4u32,
            null::<c_void>(),
        )
    };
    unsafe { *(data as *mut *mut WlProxy) = compositor };
}

unsafe extern "C" fn registry_global_remove(
    _data: *mut c_void,
    _registry: *mut WlRegistry,
    _name: u32,
) {
}

struct Weston {
    child: Child,
    runtime_dir: PathBuf,
}

impl Drop for Weston {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

fn start_weston() -> Option<Weston> {
    let runtime_dir = std::env::temp_dir().join(format!("nga-wayland-{}", std::process::id()));
    std::fs::create_dir_all(&runtime_dir).ok()?;
    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).ok()?;

    let child = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--renderer=gl",
            &format!("--socket={SOCKET}"),
            &format!("--width={WIDTH}"),
            &format!("--height={HEIGHT}"),
            "--idle-time=0",
            "--log=/dev/null",
        ])
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let socket = runtime_dir.join(SOCKET);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if socket.exists() {
            return Some(Weston { child, runtime_dir });
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    eprintln!("weston did not create its socket, skipping");
    None
}

/// Owned Wayland connection and surface pointers.
struct WaylandWindow {
    display: *mut WlDisplay,
    surface: *mut WlSurface,
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        unsafe { wl_display_disconnect(self.display) };
    }
}

unsafe fn create_window(socket_path: &Path) -> Option<WaylandWindow> {
    unsafe {
        let path = CString::new(socket_path.to_str()?).ok()?;
        let display = wl_display_connect(path.as_ptr());
        if display.is_null() {
            eprintln!("cannot connect to weston, skipping");
            return None;
        }

        // wl_display::get_registry (opcode 1) returns a new wl_registry.
        let registry = wl_proxy_marshal_flags(
            display.cast(),
            1,
            &wl_registry_interface,
            wl_proxy_get_version(display.cast()),
            0,
            null::<c_void>(),
        ) as *mut WlRegistry;
        let mut listener = WlRegistryListener {
            global: registry_global,
            global_remove: registry_global_remove,
        };
        let mut compositor: *mut WlProxy = null_mut();
        wl_proxy_add_listener(
            registry.cast(),
            (&mut listener as *mut WlRegistryListener).cast(),
            (&mut compositor as *mut *mut WlProxy).cast(),
        );
        wl_display_roundtrip(display);
        if compositor.is_null() {
            eprintln!("weston exposes no wl_compositor, skipping");
            wl_display_disconnect(display);
            return None;
        }

        // wl_compositor::create_surface (opcode 0) returns a new wl_surface.
        let surface = wl_proxy_marshal_flags(
            compositor,
            0,
            &wl_surface_interface,
            wl_proxy_get_version(compositor),
            0,
            null::<c_void>(),
        ) as *mut WlSurface;
        wl_display_flush(display);
        if surface.is_null() {
            wl_display_disconnect(display);
            return None;
        }

        Some(WaylandWindow { display, surface })
    }
}

#[test]
fn presents_to_a_wayland_surface() {
    let Some(weston) = start_weston() else {
        return;
    };
    let socket_path = weston.runtime_dir.join(SOCKET);
    let Some(window) = (unsafe { create_window(&socket_path) }) else {
        return;
    };

    unsafe {
        let description = sys::DeviceDesc {
            window: window.surface.cast(),
            display: window.display.cast(),
            swapchain_format: sys::Format::bgra8_srgb,
            swapchain_extent: sys::uint32x2::new(WIDTH, HEIGHT),
            desired_swapchain_image_count: 2,
            ..sys::DeviceDesc::default()
        };
        let mut init = sys::DeviceInit::default();
        sys::create_device(&description, &mut init);
        assert_eq!(
            init.error,
            sys::Error::none,
            "Wayland device creation failed"
        );
        let device = init.device;
        assert!(!device.is_null());

        let mut extent = sys::uint32x2::default();
        sys::get_drawable_extent(device, &mut extent);
        assert_eq!(
            (extent.x, extent.y),
            (WIDTH, HEIGHT),
            "variable-size surface reports the requested extent"
        );

        let mut color_targets = [sys::ColorTargetDesc {
            format: sys::Format::bgra8_srgb,
            ..sys::ColorTargetDesc::default()
        }];
        let pso = sys::create_graphics_pso(
            device,
            &sys::GraphicsPSODesc {
                vertex: sys::ShaderStage {
                    code: sys::ByteSpan::from_slice(TRIANGLE_VERTEX.bytes()),
                    entry_point: c"vertexMain".as_ptr(),
                    ..sys::ShaderStage::default()
                },
                fragment: sys::ShaderStage {
                    code: sys::ByteSpan::from_slice(TRIANGLE_FRAGMENT.bytes()),
                    entry_point: c"fragmentMain".as_ptr(),
                    ..sys::ShaderStage::default()
                },
                color_targets: sys::Span::from_slice(&mut color_targets),
                ..sys::GraphicsPSODesc::default()
            },
        );
        assert!(!pso.is_null(), "pipeline creation failed");

        let pool = sys::create_command_pool(device, 0);
        let semaphore = sys::create_timeline_semaphore(device, 0);
        let mut completion = sys::TimelinePoint {
            semaphore,
            value: 0,
        };
        assert!(!pool.is_null() && !semaphore.is_null());

        let mut presented = 0u32;
        for _ in 0..8 {
            sys::reset_command_pool(pool);
            let commands = sys::begin_commands(pool);
            let mut frame = sys::SwapchainFrame::default();
            sys::acquire(commands, &mut frame);
            if frame.render_view.is_null() {
                sys::end_commands(commands);
                continue;
            }
            assert_eq!(
                (frame.extent.x, frame.extent.y),
                (WIDTH, HEIGHT),
                "acquired frame extent"
            );

            let mut colors = [sys::ColorAttachment {
                render_view: frame.render_view,
                load: sys::LoadOp::clear,
                store: sys::StoreOp::store,
                clear: sys::ClearColor {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                    w: 1.0,
                },
            }];
            let rendering = sys::RenderingDesc {
                colors: sys::Span::from_slice(&mut colors),
                ..sys::RenderingDesc::default()
            };
            sys::begin_render_pass(commands, &rendering, sys::RenderingFlags::none);
            sys::bind_pso(commands, pso);
            sys::draw(commands, null(), 3, 1, 0, 0);
            sys::end_render_pass(commands);
            sys::end_commands(commands);

            completion.value += 1;
            let mut commands = [commands];
            let submit = sys::SubmitDesc {
                commands: sys::Span::from_slice(&mut commands),
                waits: sys::Span::empty(),
                completion,
            };
            sys::submit_and_present(device, &submit);
            if completion.value >= 2 {
                sys::wait_timeline(sys::TimelinePoint {
                    semaphore,
                    value: completion.value - 1,
                });
            }
            presented += 1;
        }

        assert!(presented >= 3, "presented {presented} frames");
        sys::wait_idle(device);

        sys::destroy_command_pool(pool);
        sys::destroy_timeline_semaphore(semaphore);
        sys::destroy_pso(pso);
        sys::destroy_device(device);
    }

    drop(window);
}
