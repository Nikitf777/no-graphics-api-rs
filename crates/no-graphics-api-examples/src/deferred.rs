//! Port of `examples/deferred_renderer/deferred_renderer.cpp`: GPU simulation, mesh-shader G-buffer,
//! and deferred lighting with two frames in flight and a delete queue for retired G-buffers.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat4, Vec3};
use no_graphics_api_sys as sys;
use no_graphics_api_sys::GpuCpuRange;
use no_graphics_api_utility::shader_types::{float2, float3, float3x4, float4, float4x4};
use no_graphics_api_utility::{BumpAllocator, DeleteQueue, PlacedTexture, TextureAllocator};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::math::{look_at_rh, perspective_rh_zo, to_float3, to_float4, to_row_major};
use crate::support::{self, AlignedShader};

static SIMULATION_SHADER: AlignedShader<
    { include_bytes!("../shaders/simulation.comp.spv").len() },
> = AlignedShader::from_bytes(include_bytes!("../shaders/simulation.comp.spv"));
static GBUFFER_MESH_SHADER: AlignedShader<{ include_bytes!("../shaders/gbuffer.mesh.spv").len() }> =
    AlignedShader::from_bytes(include_bytes!("../shaders/gbuffer.mesh.spv"));
static GBUFFER_FRAGMENT_SHADER: AlignedShader<
    { include_bytes!("../shaders/gbuffer.frag.spv").len() },
> = AlignedShader::from_bytes(include_bytes!("../shaders/gbuffer.frag.spv"));
static LIGHTING_VERTEX_SHADER: AlignedShader<
    { include_bytes!("../shaders/deferred.vert.spv").len() },
> = AlignedShader::from_bytes(include_bytes!("../shaders/deferred.vert.spv"));
static LIGHTING_FRAGMENT_SHADER: AlignedShader<
    { include_bytes!("../shaders/deferred.frag.spv").len() },
> = AlignedShader::from_bytes(include_bytes!("../shaders/deferred.frag.spv"));

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const SWAPCHAIN_FORMAT: sys::Format = sys::Format::bgra8_srgb;

const OBJECT_GRID_WIDTH: u32 = 512;
const OBJECT_COUNT: u32 = OBJECT_GRID_WIDTH * OBJECT_GRID_WIDTH;
const SIMULATION_THREAD_COUNT: u32 = 256;
const GBUFFER_THREAD_COUNT: u32 = 32;
const GBUFFER_TEXTURE_COUNT: u32 = 3;
const FRAMES_IN_FLIGHT: u32 = 2;
const CENTRAL_GRAVITY: f32 = 81000.0;
const GRAVITY_SOFTENING: f32 = 12.0;
const MINIMUM_ORBIT_RADIUS: f32 = 54.0;
const MAXIMUM_ORBIT_RADIUS: f32 = 240.0;
const CUBE_SCALE: f32 = 0.42;
const CUBE_ROTATION_SPEED: f32 = 0.5;
const DATA_HEAP_SIZE: u64 = 16 * 1024 * 1024;
const TEXTURE_HEAP_SIZE: u64 = 256 * 1024 * 1024;
const MAX_TEXTURES: u32 = 64;
const CAMERA_POSITION: Vec3 = Vec3::new(260.0, 210.0, 300.0);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ObjectData {
    position: float3,
    velocity: float3,
}

#[repr(C)]
struct SimulationRoot {
    objects: *const ObjectData,
    delta_seconds: f32,
    root_alignment_padding: u32,
}

#[repr(C)]
struct GBufferRoot {
    objects: *const ObjectData,
    root_alignment_padding: [u32; 2],
    view_projection: float4x4,
    orientation: float3x4,
}

#[repr(C)]
struct DeferredLightingRoot {
    camera_position: float4,
    ray_center: float4,
    ray_horizontal: float4,
    ray_vertical: float4,
    depth_linearize: float2,
    gbuffer_pixel_scale: float2,
    gbuffer_texture_base: u32,
    root_alignment_padding: [u32; 3],
}

const _: () = assert!(core::mem::size_of::<ObjectData>() == 24);
const _: () = assert!(core::mem::size_of::<SimulationRoot>() == 16);
const _: () = assert!(core::mem::size_of::<GBufferRoot>() == 128);
const _: () = assert!(core::mem::size_of::<DeferredLightingRoot>() == 96);

fn random_signed(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
}

fn initialize_object_data(objects: &mut [ObjectData]) {
    let minimum_radius_squared = MINIMUM_ORBIT_RADIUS * MINIMUM_ORBIT_RADIUS;
    let maximum_radius_squared = MAXIMUM_ORBIT_RADIUS * MAXIMUM_ORBIT_RADIUS;
    let softening_squared = GRAVITY_SOFTENING * GRAVITY_SOFTENING;
    let mut random_state = 0x1234_5678u32;

    for object in objects {
        let mut position;
        let mut radius_squared;
        loop {
            position = Vec3::new(
                random_signed(&mut random_state) * MAXIMUM_ORBIT_RADIUS,
                random_signed(&mut random_state) * MAXIMUM_ORBIT_RADIUS,
                random_signed(&mut random_state) * MAXIMUM_ORBIT_RADIUS,
            );
            radius_squared = position.length_squared();
            if radius_squared >= minimum_radius_squared && radius_squared <= maximum_radius_squared
            {
                break;
            }
        }

        let tangent;
        loop {
            let random_axis = Vec3::new(
                random_signed(&mut random_state),
                random_signed(&mut random_state),
                random_signed(&mut random_state),
            );
            let axis_length_squared = random_axis.length_squared();
            let candidate = position.cross(random_axis);
            if (0.01..=1.0).contains(&axis_length_squared) && candidate.length_squared() >= 0.01 {
                tangent = candidate.normalize();
                break;
            }
        }

        let inverse_softened_distance = (radius_squared + softening_squared).sqrt().recip();
        let circular_speed = (CENTRAL_GRAVITY
            * radius_squared
            * inverse_softened_distance
            * inverse_softened_distance
            * inverse_softened_distance)
            .sqrt();
        let speed_scale = 0.985 + random_signed(&mut random_state) * 0.045;

        *object = ObjectData {
            position: to_float3(position),
            velocity: to_float3(tangent * (circular_speed * speed_scale)),
        };
    }
}

#[derive(Clone, Copy, Default)]
struct GBuffer {
    albedo: PlacedTexture,
    normal_roughness: PlacedTexture,
    depth: PlacedTexture,
    albedo_view: *mut sys::RenderView,
    normal_roughness_view: *mut sys::RenderView,
    depth_view: *mut sys::RenderView,
    width: u32,
    height: u32,
}

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
    completion: sys::TimelinePoint,
    simulation_pso: *mut sys::PSO,
    gbuffer_pso: *mut sys::PSO,
    lighting_pso: *mut sys::PSO,
    pools: [*mut sys::CommandPool; 2],
    frame: sys::SwapchainFrame,
    data_heap: sys::GpuHeap,
    frame_ranges: [GpuCpuRange<u8>; 2],
    objects: GpuCpuRange<ObjectData>,
    texture_heap: sys::TextureHeap,
    allocator: TextureAllocator,
    descriptors: *mut sys::TextureDescriptorHeap,
    descriptor_row: u32,
    gbuffer: GBuffer,
    delete_queue: DeleteQueue,
    retired: Rc<RefCell<Vec<GBuffer>>>,
    rotation: f32,
    last_time: Instant,
}

impl State {
    unsafe fn new(window: &Window) -> Option<Self> {
        unsafe {
            let device = support::create_device(window, SWAPCHAIN_FORMAT)?;

            let simulation_pso = sys::create_compute_pso(
                device,
                &sys::ShaderStage {
                    code: sys::ByteSpan::from_slice(SIMULATION_SHADER.bytes()),
                    entry_point: c"computeMain".as_ptr(),
                    threadgroup_size: sys::uint32x3::new(SIMULATION_THREAD_COUNT, 1, 1),
                },
            );
            let mut gbuffer_targets = [
                sys::ColorTargetDesc {
                    format: sys::Format::rgba8_unorm,
                    ..sys::ColorTargetDesc::default()
                },
                sys::ColorTargetDesc {
                    format: sys::Format::rgba16_float,
                    ..sys::ColorTargetDesc::default()
                },
            ];
            let gbuffer_pso = sys::create_mesh_pso(
                device,
                &sys::MeshPSODesc {
                    mesh: sys::ShaderStage {
                        code: sys::ByteSpan::from_slice(GBUFFER_MESH_SHADER.bytes()),
                        entry_point: c"meshMain".as_ptr(),
                        threadgroup_size: sys::uint32x3::new(GBUFFER_THREAD_COUNT, 1, 1),
                    },
                    fragment: support::shader_stage(
                        GBUFFER_FRAGMENT_SHADER.bytes(),
                        c"fragmentMain",
                    ),
                    color_targets: sys::Span::from_slice(&mut gbuffer_targets),
                    depth_format: sys::Format::d32_float,
                    rasterization: sys::RasterizationState {
                        cull: sys::CullMode::clockwise,
                        ..sys::RasterizationState::default()
                    },
                    ..sys::MeshPSODesc::default()
                },
            );
            let mut lighting_targets = [sys::ColorTargetDesc {
                format: SWAPCHAIN_FORMAT,
                ..sys::ColorTargetDesc::default()
            }];
            let lighting_pso = sys::create_graphics_pso(
                device,
                &sys::GraphicsPSODesc {
                    vertex: support::shader_stage(LIGHTING_VERTEX_SHADER.bytes(), c"vertexMain"),
                    fragment: support::shader_stage(
                        LIGHTING_FRAGMENT_SHADER.bytes(),
                        c"fragmentMain",
                    ),
                    color_targets: sys::Span::from_slice(&mut lighting_targets),
                    ..sys::GraphicsPSODesc::default()
                },
            );
            if simulation_pso.is_null() || gbuffer_pso.is_null() || lighting_pso.is_null() {
                eprintln!("pipeline creation failed (mesh shader support required)");
                sys::destroy_pso(lighting_pso);
                sys::destroy_pso(gbuffer_pso);
                sys::destroy_pso(simulation_pso);
                sys::destroy_device(device);
                return None;
            }

            let mut data_heap = sys::GpuHeap::default();
            let mut texture_heap = sys::TextureHeap::default();
            sys::create_gpu_heap(
                device,
                DATA_HEAP_SIZE,
                sys::MemoryType::cpu_visible,
                &mut data_heap,
            );
            sys::create_texture_heap(device, TEXTURE_HEAP_SIZE, &mut texture_heap);
            let descriptors = sys::create_texture_descriptor_heap(
                device,
                GBUFFER_TEXTURE_COUNT * FRAMES_IN_FLIGHT,
            );
            let semaphore = sys::create_timeline_semaphore(device, 0);
            let pools = [
                sys::create_command_pool(device, 0),
                sys::create_command_pool(device, 0),
            ];
            if data_heap.owner.is_null()
                || texture_heap.owner.is_null()
                || descriptors.is_null()
                || semaphore.is_null()
            {
                sys::destroy_gpu_heap(&data_heap);
                sys::destroy_texture_heap(&texture_heap);
                sys::destroy_texture_descriptor_heap(descriptors);
                if !semaphore.is_null() {
                    sys::destroy_timeline_semaphore(semaphore);
                }
                for pool in pools {
                    if !pool.is_null() {
                        sys::destroy_command_pool(pool);
                    }
                }
                sys::destroy_pso(lighting_pso);
                sys::destroy_pso(gbuffer_pso);
                sys::destroy_pso(simulation_pso);
                sys::destroy_device(device);
                return None;
            }

            let mut data_allocator = BumpAllocator::new(data_heap.range);
            let frame_ranges = [
                data_allocator.allocate(4096)?,
                data_allocator.allocate(4096)?,
            ];
            let objects = data_allocator.allocate_typed::<ObjectData>(u64::from(OBJECT_COUNT))?;
            initialize_object_data(core::slice::from_raw_parts_mut(
                objects.cpu,
                OBJECT_COUNT as usize,
            ));

            let allocator = TextureAllocator::new(device, texture_heap, MAX_TEXTURES);

            Some(Self {
                device,
                completion: sys::TimelinePoint {
                    semaphore,
                    value: 0,
                },
                simulation_pso,
                gbuffer_pso,
                lighting_pso,
                pools,
                frame: sys::SwapchainFrame::default(),
                data_heap,
                frame_ranges,
                objects,
                texture_heap,
                allocator,
                descriptors,
                descriptor_row: 0,
                gbuffer: GBuffer::default(),
                delete_queue: DeleteQueue::new(semaphore, FRAMES_IN_FLIGHT),
                retired: Rc::new(RefCell::new(Vec::new())),
                rotation: 0.0,
                last_time: Instant::now(),
            })
        }
    }

    unsafe fn destroy_gbuffer(&mut self, gbuffer: GBuffer) {
        unsafe {
            for view in [
                gbuffer.albedo_view,
                gbuffer.normal_roughness_view,
                gbuffer.depth_view,
            ] {
                if !view.is_null() {
                    sys::destroy_render_view(view);
                }
            }
            self.allocator.free(gbuffer.depth);
            self.allocator.free(gbuffer.normal_roughness);
            self.allocator.free(gbuffer.albedo);
        }
    }

    unsafe fn recreate_gbuffer(
        &mut self,
        commands: *mut sys::CommandBuffer,
        width: u32,
        height: u32,
    ) -> Option<()> {
        unsafe {
            let mut gbuffer = GBuffer {
                albedo: self.allocator.allocate(&sys::TextureDesc {
                    extent: sys::uint32x3::new(width, height, 1),
                    usage: sys::TextureUsage::sampled | sys::TextureUsage::color_attachment,
                    ..sys::TextureDesc::default()
                })?,
                normal_roughness: self.allocator.allocate(&sys::TextureDesc {
                    extent: sys::uint32x3::new(width, height, 1),
                    format: sys::Format::rgba16_float,
                    usage: sys::TextureUsage::sampled | sys::TextureUsage::color_attachment,
                    ..sys::TextureDesc::default()
                })?,
                depth: self.allocator.allocate(&sys::TextureDesc {
                    extent: sys::uint32x3::new(width, height, 1),
                    format: sys::Format::d32_float,
                    usage: sys::TextureUsage::sampled | sys::TextureUsage::depth_stencil_attachment,
                    ..sys::TextureDesc::default()
                })?,
                width,
                height,
                ..GBuffer::default()
            };

            sys::initialize_texture(commands, gbuffer.albedo.texture);
            sys::initialize_texture(commands, gbuffer.normal_roughness.texture);
            sys::initialize_texture(commands, gbuffer.depth.texture);
            gbuffer.albedo_view =
                sys::create_render_view(gbuffer.albedo.texture, &sys::RenderViewDesc::default());
            gbuffer.normal_roughness_view = sys::create_render_view(
                gbuffer.normal_roughness.texture,
                &sys::RenderViewDesc::default(),
            );
            gbuffer.depth_view =
                sys::create_render_view(gbuffer.depth.texture, &sys::RenderViewDesc::default());
            if gbuffer.albedo_view.is_null()
                || gbuffer.normal_roughness_view.is_null()
                || gbuffer.depth_view.is_null()
            {
                return None;
            }

            let base = self.descriptor_row * GBUFFER_TEXTURE_COUNT;
            sys::write_texture_descriptor(
                self.descriptors,
                base,
                gbuffer.albedo.texture,
                sys::TextureDescriptorType::sampled,
                &sys::TextureDescriptorDesc::default(),
            );
            sys::write_texture_descriptor(
                self.descriptors,
                base + 1,
                gbuffer.normal_roughness.texture,
                sys::TextureDescriptorType::sampled,
                &sys::TextureDescriptorDesc::default(),
            );
            sys::write_texture_descriptor(
                self.descriptors,
                base + 2,
                gbuffer.depth.texture,
                sys::TextureDescriptorType::sampled,
                &sys::TextureDescriptorDesc::default(),
            );

            self.gbuffer = gbuffer;
            Some(())
        }
    }

    unsafe fn render(&mut self, window: &Window) {
        unsafe {
            if self.completion.value >= u64::from(FRAMES_IN_FLIGHT) {
                sys::wait_timeline(sys::TimelinePoint {
                    semaphore: self.completion.semaphore,
                    value: self.completion.value - u64::from(FRAMES_IN_FLIGHT) + 1,
                });
            }
            self.delete_queue.tick();

            // Free G-buffers retired by the delete queue, now that the allocator can be borrowed.
            let ready: Vec<GBuffer> = self.retired.borrow_mut().drain(..).collect();
            for gbuffer in ready {
                self.destroy_gbuffer(gbuffer);
            }

            let mut frame_allocator = BumpAllocator::new(
                self.frame_ranges[(self.completion.value % u64::from(FRAMES_IN_FLIGHT)) as usize],
            );
            let pool = self.pools[(self.completion.value % u64::from(FRAMES_IN_FLIGHT)) as usize];
            sys::reset_command_pool(pool);
            let commands = sys::begin_commands(pool);
            sys::acquire(commands, &mut self.frame);
            if self.frame.render_view.is_null() {
                sys::end_commands(commands);
                return;
            }
            let extent = self.frame.extent;

            // Window resize: retire the current G-buffer and recreate it at the new extent.
            if extent.x != self.gbuffer.width || extent.y != self.gbuffer.height {
                if !self.gbuffer.albedo.texture.is_null() {
                    let gbuffer = self.gbuffer;
                    let retired = Rc::clone(&self.retired);
                    self.delete_queue.defer(self.completion.value, move || {
                        retired.borrow_mut().push(gbuffer)
                    });
                    self.gbuffer = GBuffer::default();
                    self.descriptor_row = (self.descriptor_row + 1) % FRAMES_IN_FLIGHT;
                }
                if self
                    .recreate_gbuffer(commands, extent.x, extent.y)
                    .is_none()
                {
                    sys::end_commands(commands);
                    return;
                }
            }

            sys::set_texture_descriptor_heap(commands, self.descriptors);

            // Simulation.
            let now = Instant::now();
            let delta_seconds = (now - self.last_time).as_secs_f32();
            self.last_time = now;

            sys::barrier(
                commands,
                sys::Stage::compute | sys::Stage::mesh,
                sys::Access::shader_write | sys::Access::shader_read,
                sys::Stage::compute,
                sys::Access::shader_read | sys::Access::shader_write,
            );
            sys::bind_pso(commands, self.simulation_pso);
            let simulation_root = frame_allocator
                .allocate_typed::<SimulationRoot>(1)
                .expect("frame data");
            simulation_root.cpu.write(SimulationRoot {
                objects: self.objects.gpu.cast_const(),
                delta_seconds,
                root_alignment_padding: 0,
            });
            sys::dispatch(
                commands,
                simulation_root.gpu.cast(),
                sys::uint32x3::new(OBJECT_COUNT / SIMULATION_THREAD_COUNT, 1, 1),
            );

            // G-buffer.
            sys::barrier(
                commands,
                sys::Stage::compute,
                sys::Access::shader_write,
                sys::Stage::mesh,
                sys::Access::shader_read,
            );
            sys::barrier(
                commands,
                sys::Stage::fragment | sys::Stage::color_output | sys::Stage::depth_stencil_tests,
                sys::Access::shader_read
                    | sys::Access::color_write
                    | sys::Access::depth_stencil_write,
                sys::Stage::color_output | sys::Stage::depth_stencil_tests,
                sys::Access::color_write | sys::Access::depth_stencil_write,
            );
            let mut gbuffer_colors = [
                sys::ColorAttachment {
                    render_view: self.gbuffer.albedo_view,
                    load: sys::LoadOp::clear,
                    ..sys::ColorAttachment::default()
                },
                sys::ColorAttachment {
                    render_view: self.gbuffer.normal_roughness_view,
                    load: sys::LoadOp::clear,
                    ..sys::ColorAttachment::default()
                },
            ];
            let gbuffer_pass = sys::RenderingDesc {
                colors: sys::Span::from_slice(&mut gbuffer_colors),
                depth: sys::DepthAttachment {
                    render_view: self.gbuffer.depth_view,
                    load: sys::LoadOp::clear,
                    ..sys::DepthAttachment::default()
                },
                ..sys::RenderingDesc::default()
            };
            sys::begin_render_pass(commands, &gbuffer_pass, sys::RenderingFlags::none);
            sys::set_depth_stencil(
                commands,
                &sys::DepthStencilState {
                    depth_test: true,
                    depth_write: true,
                    ..sys::DepthStencilState::default()
                },
            );
            sys::bind_pso(commands, self.gbuffer_pso);

            let view = look_at_rh(CAMERA_POSITION, Vec3::ZERO, Vec3::Y);
            let projection = perspective_rh_zo(
                std::f32::consts::PI / 3.0,
                extent.x as f32 / extent.y as f32,
                0.3,
                1500.0,
            );
            self.rotation += CUBE_ROTATION_SPEED * delta_seconds;
            let orientation = Mat4::from_rotation_y(self.rotation)
                * Mat4::from_rotation_x(self.rotation * 0.5)
                * Mat4::from_scale(Vec3::splat(CUBE_SCALE));
            let orientation = to_row_major(orientation);

            let gbuffer_root = frame_allocator
                .allocate_typed::<GBufferRoot>(1)
                .expect("frame data");
            gbuffer_root.cpu.write(GBufferRoot {
                objects: self.objects.gpu.cast_const(),
                root_alignment_padding: [0; 2],
                view_projection: to_row_major(projection * view),
                orientation: float3x4 {
                    rows: [
                        orientation.rows[0],
                        orientation.rows[1],
                        orientation.rows[2],
                    ],
                },
            });
            sys::draw_meshlets(
                commands,
                gbuffer_root.gpu.cast(),
                sys::uint32x3::new(OBJECT_GRID_WIDTH, OBJECT_GRID_WIDTH, 1),
            );
            sys::end_render_pass(commands);

            // Deferred lighting.
            sys::barrier(
                commands,
                sys::Stage::color_output | sys::Stage::depth_stencil_tests,
                sys::Access::color_write | sys::Access::depth_stencil_write,
                sys::Stage::fragment,
                sys::Access::shader_read,
            );
            let mut lighting_colors = [sys::ColorAttachment {
                render_view: self.frame.render_view,
                load: sys::LoadOp::discard,
                store: sys::StoreOp::store,
                clear: sys::ClearColor::default(),
            }];
            let lighting_pass = sys::RenderingDesc {
                colors: sys::Span::from_slice(&mut lighting_colors),
                ..sys::RenderingDesc::default()
            };
            sys::begin_render_pass(commands, &lighting_pass, sys::RenderingFlags::none);
            sys::bind_pso(commands, self.lighting_pso);

            let projection_rows = to_row_major(projection);
            let view_right = view.row(0).truncate();
            let view_up = view.row(1).truncate();
            let view_forward = -view.row(2).truncate();
            let root = frame_allocator
                .allocate_typed::<DeferredLightingRoot>(1)
                .expect("frame data");
            root.cpu.write(DeferredLightingRoot {
                camera_position: to_float4(CAMERA_POSITION.extend(1.0)),
                ray_center: to_float4(view_forward.extend(0.0)),
                ray_horizontal: to_float4((view_right / projection_rows.rows[0].x).extend(0.0)),
                ray_vertical: to_float4((view_up / projection_rows.rows[1].y).extend(0.0)),
                depth_linearize: float2::new(
                    -projection_rows.rows[2].w,
                    -projection_rows.rows[2].z,
                ),
                gbuffer_pixel_scale: float2::new(1.0, 1.0),
                gbuffer_texture_base: self.descriptor_row * GBUFFER_TEXTURE_COUNT,
                root_alignment_padding: [0; 3],
            });
            sys::draw(commands, root.gpu.cast(), 3, 1, 0, 0);
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
            self.delete_queue.drain();
            let ready: Vec<GBuffer> = self.retired.borrow_mut().drain(..).collect();
            for gbuffer in ready {
                self.destroy_gbuffer(gbuffer);
            }
            self.destroy_gbuffer(self.gbuffer);
            self.gbuffer = GBuffer::default();
            for pool in self.pools {
                sys::destroy_command_pool(pool);
            }
            sys::destroy_timeline_semaphore(self.completion.semaphore);
            sys::destroy_pso(self.lighting_pso);
            sys::destroy_pso(self.gbuffer_pso);
            sys::destroy_pso(self.simulation_pso);
            sys::destroy_texture_heap(&self.texture_heap);
            sys::destroy_gpu_heap(&self.data_heap);
            sys::destroy_texture_descriptor_heap(self.descriptors);
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
            .with_title("NoGraphicsAPI deferred renderer")
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
