//! Port of `examples/cube/cube.cpp`: spinning textured cube with a depth buffer, descriptor heaps,
//! texture upload, and per-frame root data.

use std::sync::Arc;

use glam::Mat4;
use no_graphics_api_sys as sys;
use no_graphics_api_sys::GpuCpuRange;
use no_graphics_api_utility::shader_types::{float2, float4, float4x4};
use no_graphics_api_utility::{BumpAllocator, PlacedTexture, TextureAllocator, UploadQueue};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::math::{look_at_rh, perspective_rh_zo, to_row_major};
use crate::support::{self, AlignedShader};

static VERTEX_SHADER: AlignedShader<{ include_bytes!("../shaders/cube.vert.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/cube.vert.spv"));
static FRAGMENT_SHADER: AlignedShader<{ include_bytes!("../shaders/cube.frag.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/cube.frag.spv"));
static TEXTURE_RGBA8: &[u8] = include_bytes!("../shaders/lunarg_logo_256x256.rgba8");

const WIDTH: u32 = 500;
const HEIGHT: u32 = 500;
const SWAPCHAIN_FORMAT: sys::Format = sys::Format::bgra8_srgb;
const TEXTURE_WIDTH: u32 = 256;
const TEXTURE_HEIGHT: u32 = 256;
const DATA_HEAP_SIZE: u64 = 1024 * 1024;
const TEXTURE_HEAP_SIZE: u64 = 64 * 1024 * 1024;
const MAX_TEXTURES: u32 = 16;
const RADIANS_PER_FRAME: f32 = 4.0 * std::f32::consts::PI / 180.0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CubeVertex {
    position: float4,
    uv: float2,
}

#[repr(C)]
struct CubeRootArguments {
    vertices: *const CubeVertex,
    root_alignment_padding: [u32; 2],
    transform: float4x4,
}

const _: () = assert!(core::mem::size_of::<CubeRootArguments>() == 80);
const _: () = assert!(core::mem::size_of::<CubeVertex>() == 24);

const CUBE_VERTICES: [CubeVertex; 24] = [
    CubeVertex {
        position: float4::new(-1.0, -1.0, -1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, -1.0, 1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, 1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, -1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
    CubeVertex {
        position: float4::new(-1.0, -1.0, -1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, -1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, -1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, -1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, -1.0, -1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, -1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, 1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, -1.0, 1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, -1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, 1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, 1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, -1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, -1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, 1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, 1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, -1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(-1.0, -1.0, 1.0, 1.0),
        uv: float2::new(0.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, -1.0, 1.0, 1.0),
        uv: float2::new(1.0, 1.0),
    },
    CubeVertex {
        position: float4::new(1.0, 1.0, 1.0, 1.0),
        uv: float2::new(1.0, 0.0),
    },
    CubeVertex {
        position: float4::new(-1.0, 1.0, 1.0, 1.0),
        uv: float2::new(0.0, 0.0),
    },
];

const CUBE_INDICES: [u16; 36] = [
    0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4, 8, 9, 10, 10, 11, 8, 12, 13, 14, 14, 15, 12, 16, 17, 18,
    18, 19, 16, 20, 21, 22, 22, 23, 20,
];

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
    data_heap: sys::GpuHeap,
    frame_ranges: [GpuCpuRange<u8>; 2],
    vertex_allocation: GpuCpuRange<CubeVertex>,
    index_allocation: GpuCpuRange<u16>,
    texture_heap: sys::TextureHeap,
    allocator: TextureAllocator,
    texture: PlacedTexture,
    depth: PlacedTexture,
    depth_view: *mut sys::RenderView,
    depth_extent: sys::uint32x2,
    texture_descriptors: *mut sys::TextureDescriptorHeap,
    sampler_descriptors: *mut sys::SamplerDescriptorHeap,
    rotation: f32,
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
                    depth_format: sys::Format::d32_float,
                    rasterization: sys::RasterizationState {
                        cull: sys::CullMode::clockwise,
                        ..sys::RasterizationState::default()
                    },
                    ..sys::GraphicsPSODesc::default()
                },
            );
            if pso.is_null() {
                sys::destroy_device(device);
                return None;
            }

            let mut data_heap = sys::GpuHeap::default();
            sys::create_gpu_heap(
                device,
                DATA_HEAP_SIZE,
                sys::MemoryType::cpu_visible,
                &mut data_heap,
            );
            let mut texture_heap = sys::TextureHeap::default();
            sys::create_texture_heap(device, TEXTURE_HEAP_SIZE, &mut texture_heap);
            if data_heap.owner.is_null() || texture_heap.owner.is_null() {
                sys::destroy_gpu_heap(&data_heap);
                sys::destroy_texture_heap(&texture_heap);
                sys::destroy_pso(pso);
                sys::destroy_device(device);
                return None;
            }

            let mut data_allocator = BumpAllocator::new(data_heap.range);
            let frame_ranges = [
                data_allocator.allocate(4096)?,
                data_allocator.allocate(4096)?,
            ];
            let vertex_allocation =
                data_allocator.allocate_typed::<CubeVertex>(CUBE_VERTICES.len() as u64)?;
            let index_allocation =
                data_allocator.allocate_typed::<u16>(CUBE_INDICES.len() as u64)?;
            core::ptr::copy_nonoverlapping(
                CUBE_VERTICES.as_ptr(),
                vertex_allocation.cpu,
                CUBE_VERTICES.len(),
            );
            core::ptr::copy_nonoverlapping(
                CUBE_INDICES.as_ptr(),
                index_allocation.cpu,
                CUBE_INDICES.len(),
            );

            let mut allocator = TextureAllocator::new(device, texture_heap, MAX_TEXTURES);
            let texture = allocator.allocate(&sys::TextureDesc {
                extent: sys::uint32x3::new(TEXTURE_WIDTH, TEXTURE_HEIGHT, 1),
                format: sys::Format::rgba8_srgb,
                usage: sys::TextureUsage::sampled | sys::TextureUsage::transfer_destination,
                ..sys::TextureDesc::default()
            })?;

            let texture_descriptors = sys::create_texture_descriptor_heap(device, 1);
            let sampler_descriptors = sys::create_sampler_descriptor_heap(device, 1);
            let semaphore = sys::create_timeline_semaphore(device, 0);
            let pools = [
                sys::create_command_pool(device, 0),
                sys::create_command_pool(device, 0),
            ];

            let mut state = Self {
                device,
                pso,
                completion: sys::TimelinePoint {
                    semaphore,
                    value: 0,
                },
                pools,
                frame: sys::SwapchainFrame::default(),
                data_heap,
                frame_ranges,
                vertex_allocation,
                index_allocation,
                texture_heap,
                allocator,
                texture,
                depth: PlacedTexture::default(),
                depth_view: core::ptr::null_mut(),
                depth_extent: sys::uint32x2::default(),
                texture_descriptors,
                sampler_descriptors,
                rotation: 0.0,
            };

            if texture_descriptors.is_null()
                || sampler_descriptors.is_null()
                || semaphore.is_null()
                || pools[0].is_null()
                || pools[1].is_null()
            {
                return None;
            }

            // One-shot upload: initialize the texture, write descriptors, then copy the image.
            let commands = sys::begin_commands(pools[0]);
            sys::initialize_texture(commands, texture.texture);
            sys::write_texture_descriptor(
                texture_descriptors,
                0,
                texture.texture,
                sys::TextureDescriptorType::sampled,
                &sys::TextureDescriptorDesc::default(),
            );
            sys::write_sampler_descriptor(
                sampler_descriptors,
                0,
                &sys::SamplerDesc {
                    min_filter: sys::Filter::nearest,
                    mag_filter: sys::Filter::nearest,
                    address_u: sys::AddressMode::clamp_to_edge,
                    address_v: sys::AddressMode::clamp_to_edge,
                    ..sys::SamplerDesc::default()
                },
            );
            sys::end_commands(commands);
            state.completion.value += 1;
            let mut commands = [commands];
            let submit = sys::SubmitDesc {
                commands: sys::Span::from_slice(&mut commands),
                waits: sys::Span::empty(),
                completion: state.completion,
            };
            sys::submit(device, &submit, 0);

            let mut uploads = UploadQueue::new(device, TEXTURE_RGBA8.len() as u64, 0, 2)?;
            uploads.upload_texture(
                texture.texture,
                TEXTURE_RGBA8,
                &sys::TextureCopyDesc::default(),
            );
            uploads.wait();
            uploads.destroy();
            sys::reset_command_pool(pools[0]);

            Some(state)
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

            let mut frame_allocator =
                BumpAllocator::new(self.frame_ranges[(self.completion.value % 2) as usize]);
            let pool = self.pools[(self.completion.value % 2) as usize];
            sys::reset_command_pool(pool);
            let commands = sys::begin_commands(pool);
            sys::acquire(commands, &mut self.frame);
            if self.frame.render_view.is_null() {
                sys::end_commands(commands);
                return;
            }

            // Window resize: recreate the depth target for the new drawable extent.
            if self.frame.extent != self.depth_extent {
                if !self.depth.texture.is_null() {
                    sys::wait_timeline(self.completion);
                }
                if !self.depth_view.is_null() {
                    sys::destroy_render_view(self.depth_view);
                    self.depth_view = core::ptr::null_mut();
                }
                self.allocator.free(self.depth);
                self.depth = PlacedTexture::default();
                if let Some(depth) = self.allocator.allocate(&sys::TextureDesc {
                    extent: sys::uint32x3::new(self.frame.extent.x, self.frame.extent.y, 1),
                    format: sys::Format::d32_float,
                    usage: sys::TextureUsage::depth_stencil_attachment,
                    ..sys::TextureDesc::default()
                }) {
                    sys::initialize_texture(commands, depth.texture);
                    self.depth_view =
                        sys::create_render_view(depth.texture, &sys::RenderViewDesc::default());
                    self.depth = depth;
                    self.depth_extent = self.frame.extent;
                }
            }

            sys::set_texture_descriptor_heap(commands, self.texture_descriptors);
            sys::set_sampler_descriptor_heap(commands, self.sampler_descriptors);
            sys::barrier(
                commands,
                sys::Stage::depth_stencil_tests,
                sys::Access::depth_stencil_write,
                sys::Stage::depth_stencil_tests,
                sys::Access::depth_stencil_write,
            );

            let mut colors = [sys::ColorAttachment {
                render_view: self.frame.render_view,
                load: sys::LoadOp::clear,
                store: sys::StoreOp::store,
                clear: sys::ClearColor {
                    x: 0.2,
                    y: 0.2,
                    z: 0.2,
                    w: 0.2,
                },
            }];
            let rendering = sys::RenderingDesc {
                colors: sys::Span::from_slice(&mut colors),
                depth: sys::DepthAttachment {
                    render_view: self.depth_view,
                    load: sys::LoadOp::clear,
                    store: sys::StoreOp::discard,
                    ..sys::DepthAttachment::default()
                },
                ..sys::RenderingDesc::default()
            };
            sys::begin_render_pass(commands, &rendering, sys::RenderingFlags::none);
            sys::set_depth_stencil(
                commands,
                &sys::DepthStencilState {
                    depth_test: true,
                    depth_write: true,
                    ..sys::DepthStencilState::default()
                },
            );
            sys::bind_pso(commands, self.pso);

            self.rotation += RADIANS_PER_FRAME;
            let aspect = self.frame.extent.x as f32 / self.frame.extent.y as f32;
            let projection = perspective_rh_zo(45.0f32.to_radians(), aspect, 0.1, 100.0);
            let view = look_at_rh(
                glam::Vec3::new(0.0, 3.0, 5.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            );
            let transform = to_row_major(projection * view * Mat4::from_rotation_y(self.rotation));

            let root = frame_allocator
                .allocate_typed::<CubeRootArguments>(1)
                .expect("frame data");
            root.cpu.write(CubeRootArguments {
                vertices: self.vertex_allocation.gpu.cast_const(),
                root_alignment_padding: [0; 2],
                transform,
            });

            let indices = sys::GpuRange {
                gpu: self.index_allocation.gpu.cast(),
                size: self.index_allocation.size,
            };
            sys::draw_indexed(
                commands,
                root.gpu.cast(),
                indices,
                sys::IndexType::uint16,
                CUBE_INDICES.len() as u32,
                1,
                0,
                0,
                0,
            );
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
            if !self.depth_view.is_null() {
                sys::destroy_render_view(self.depth_view);
            }
            self.allocator.free(self.depth);
            self.allocator.free(self.texture);
            sys::destroy_texture_heap(&self.texture_heap);
            sys::destroy_sampler_descriptor_heap(self.sampler_descriptors);
            sys::destroy_texture_descriptor_heap(self.texture_descriptors);
            sys::destroy_gpu_heap(&self.data_heap);
            sys::destroy_device(self.device);
        }
    }
}

struct App {
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
            .with_title("NoGraphicsAPI spinning textured cube")
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
