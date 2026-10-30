//! Port of `examples/triangle/triangle.cpp`: swapchain presentation with two frames in flight.

use std::sync::Arc;

use no_graphics_api_sys as sys;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::support::{self, AlignedShader};

static VERTEX_SHADER: AlignedShader<{ include_bytes!("../shaders/triangle.vert.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/triangle.vert.spv"));
static FRAGMENT_SHADER: AlignedShader<{ include_bytes!("../shaders/triangle.frag.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/triangle.frag.spv"));

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;
const SWAPCHAIN_FORMAT: sys::Format = sys::Format::bgra8_srgb;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = support::event_loop()?;
    let mut app = App {
        state: None,
        window: None,
        frames: 0,
        frame_limit: support::frame_limit(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct State {
    device: *mut sys::Device,
    pso: *mut sys::PSO,
    completion: sys::TimelinePoint,
    pools: [*mut sys::CommandPool; 2],
    frame: sys::SwapchainFrame,
}

impl State {
    unsafe fn new(window: &Window) -> Option<Self> {
        unsafe {
            let device = support::create_device(window, SWAPCHAIN_FORMAT)?;

            let mut color_targets = [sys::ColorTargetDesc {
                format: SWAPCHAIN_FORMAT,
                ..sys::ColorTargetDesc::default()
            }];
            let pso = sys::create_graphics_pso(
                device,
                &sys::GraphicsPSODesc {
                    vertex: support::shader_stage(VERTEX_SHADER.bytes(), c"vertexMain"),
                    fragment: support::shader_stage(FRAGMENT_SHADER.bytes(), c"fragmentMain"),
                    color_targets: sys::Span::from_slice(&mut color_targets),
                    ..sys::GraphicsPSODesc::default()
                },
            );
            if pso.is_null() {
                sys::destroy_device(device);
                return None;
            }

            let semaphore = sys::create_timeline_semaphore(device, 0);
            let pools = [
                sys::create_command_pool(device, 0),
                sys::create_command_pool(device, 0),
            ];
            if semaphore.is_null() || pools[0].is_null() || pools[1].is_null() {
                for pool in pools {
                    if !pool.is_null() {
                        sys::destroy_command_pool(pool);
                    }
                }
                if !semaphore.is_null() {
                    sys::destroy_timeline_semaphore(semaphore);
                }
                sys::destroy_pso(pso);
                sys::destroy_device(device);
                return None;
            }

            Some(Self {
                device,
                pso,
                completion: sys::TimelinePoint {
                    semaphore,
                    value: 0,
                },
                pools,
                frame: sys::SwapchainFrame::default(),
            })
        }
    }

    unsafe fn render(&mut self, window: &Window) {
        unsafe {
            if self.completion.value >= 2 {
                sys::wait_timeline(sys::TimelinePoint {
                    semaphore: self.completion.semaphore,
                    value: self.completion.value - 1,
                });
            }

            let pool = self.pools[(self.completion.value % 2) as usize];
            sys::reset_command_pool(pool);
            let commands = sys::begin_commands(pool);
            sys::acquire(commands, &mut self.frame);
            if self.frame.render_view.is_null() {
                sys::end_commands(commands);
                return;
            }

            let mut colors = [sys::ColorAttachment {
                render_view: self.frame.render_view,
                load: sys::LoadOp::clear,
                store: sys::StoreOp::store,
                clear: sys::ClearColor::default(),
            }];
            let rendering = sys::RenderingDesc {
                colors: sys::Span::from_slice(&mut colors),
                ..sys::RenderingDesc::default()
            };
            sys::begin_render_pass(commands, &rendering, sys::RenderingFlags::none);
            sys::bind_pso(commands, self.pso);
            sys::draw(commands, core::ptr::null(), 3, 1, 0, 0);
            sys::end_render_pass(commands);
            sys::end_commands(commands);

            self.completion.value += 1;
            let mut commands = [commands];
            let submit = sys::SubmitDesc {
                commands: sys::Span::from_slice(&mut commands),
                waits: sys::Span::empty(),
                completion: self.completion,
            };
            window.pre_present_notify();
            sys::submit_and_present(self.device, &submit);
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            sys::wait_idle(self.device);
            sys::destroy_command_pool(self.pools[1]);
            sys::destroy_command_pool(self.pools[0]);
            sys::destroy_timeline_semaphore(self.completion.semaphore);
            sys::destroy_pso(self.pso);
            sys::destroy_device(self.device);
        }
    }
}

struct App {
    // Declared before the window: the device must not outlive it.
    state: Option<State>,
    window: Option<Arc<Window>>,
    frames: u64,
    frame_limit: Option<u64>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("NoGraphicsAPI triangle")
            .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("window creation failed: {error}");
                event_loop.exit();
                return;
            }
        };

        self.state = unsafe { State::new(&window) };
        if self.state.is_none() {
            event_loop.exit();
            return;
        }
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_ref() {
                    unsafe { support::set_extent(state.device, size.width, size.height) };
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(state), Some(window)) = (self.state.as_mut(), self.window.as_ref()) {
                    unsafe { state.render(window) };
                    self.frames += 1;
                    if self.frame_limit.is_some_and(|limit| self.frames >= limit) {
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
