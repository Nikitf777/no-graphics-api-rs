//! Exercises the C ABI against a real backend. Skips when no Vulkan device is available.

use std::ffi::CStr;
use std::os::raw::c_char;

use no_graphics_api_sys as gpu;

unsafe extern "C" fn on_error(message: *const c_char, fatal: bool) {
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    eprintln!("NoGraphicsAPI error (fatal: {fatal}): {text}");
}

#[test]
fn texture_format_info_matches_header() {
    unsafe {
        let mut info = gpu::TextureFormatInfo::default();
        gpu::get_texture_format_info(gpu::Format::bc7_srgb, &mut info);
        assert_eq!(info.block_extent, gpu::uint32x2::new(4, 4));
        assert_eq!(info.bytes_per_block, 16);
        assert!(!info.depth && !info.stencil);

        gpu::get_texture_format_info(gpu::Format::d32_float_s8_uint, &mut info);
        assert_eq!(info.block_extent, gpu::uint32x2::new(1, 1));
        assert_eq!(info.bytes_per_block, 8);
        assert!(info.depth && info.stencil);

        gpu::get_texture_format_info(gpu::Format::rgba32_float, &mut info);
        assert_eq!(info.bytes_per_block, 16);
    }
}

#[test]
fn defaults_match_header() {
    assert_eq!(gpu::DeviceDesc::default().desired_queue_count, 1);
    assert_eq!(gpu::DeviceDesc::default().timestamp_query_count, 256);
    assert_eq!(
        gpu::DeviceDesc::default().swapchain_format,
        gpu::Format::undefined
    );
    assert_eq!(gpu::TextureDesc::default().format, gpu::Format::rgba8_unorm);
    assert_eq!(gpu::SamplerDesc::default().min_filter, gpu::Filter::linear);
    assert_eq!(gpu::ColorTargetDesc::default().write_mask, 0xf);
    assert_eq!(gpu::Viewport::default().max_depth, 1.0);
    assert_eq!(
        gpu::Stage::transfer | gpu::Stage::compute,
        gpu::Stage::from_bits(gpu::Stage::transfer.bits() | gpu::Stage::compute.bits())
    );
    assert!(gpu::TextureUsage::sampled.contains(gpu::TextureUsage::sampled));
}

#[test]
fn headless_lifecycle() {
    unsafe {
        gpu::set_error_callback(Some(on_error));

        let desc = gpu::DeviceDesc::default();
        let mut init = gpu::DeviceInit::default();
        gpu::create_device(&desc, &mut init);
        if init.device.is_null() {
            eprintln!("no GPU available, skipping (error: {:?})", init.error);
            return;
        }
        assert_eq!(init.error, gpu::Error::none);
        let device = init.device;

        let caps = &*gpu::get_device_caps(device);
        eprintln!(
            "device: {:?} queues: {} (general {}, compute {}, copy {}) push data: {} heap align: {}",
            caps.device_name_cstr().map(|name| name.to_string_lossy()),
            caps.queue_count,
            caps.general_queue_count,
            caps.compute_queue_count,
            caps.copy_queue_count,
            caps.max_push_data_size,
            caps.texture_heap_alignment
        );
        assert!(caps.queue_count >= 1);
        assert!(gpu::supports_texture_format(
            device,
            gpu::Format::rgba8_unorm,
            gpu::TextureUsage::sampled
        ));

        let mut extent = gpu::uint32x2::default();
        gpu::get_drawable_extent(device, &mut extent);

        // GPU heap round trip.
        let mut heap = gpu::GpuHeap::default();
        gpu::create_gpu_heap(device, 4096, gpu::MemoryType::cpu_visible, &mut heap);
        assert!(!heap.range.cpu.is_null());
        assert!(!heap.range.gpu.is_null());
        assert_eq!(heap.range.size, 4096);
        let mut range = gpu::GpuRange::default();
        gpu::gpu_range_of_heap(&heap, &mut range);
        assert_eq!(range.gpu, heap.range.gpu.cast());

        // Texture heap and placed texture.
        let mut texture_heap = gpu::TextureHeap::default();
        gpu::create_texture_heap(device, 1 << 20, &mut texture_heap);
        assert!(!texture_heap.owner.is_null());

        let texture_desc = gpu::TextureDesc::default();
        let mut size_align = gpu::SizeAlign::default();
        gpu::get_texture_size_align(device, &texture_desc, &mut size_align);
        assert!(size_align.size > 0 && size_align.align > 0);
        let texture = gpu::create_texture(device, &texture_desc, &texture_heap, 0);
        assert!(!texture.is_null());
        let view = gpu::create_render_view(texture, &gpu::RenderViewDesc::default());
        assert!(!view.is_null());

        // Descriptor heaps.
        let texture_descriptors = gpu::create_texture_descriptor_heap(device, 8);
        assert!(!texture_descriptors.is_null());
        gpu::write_texture_descriptor(
            texture_descriptors,
            0,
            texture,
            gpu::TextureDescriptorType::sampled,
            &gpu::TextureDescriptorDesc::default(),
        );
        gpu::copy_texture_descriptors(texture_descriptors, 0, texture_descriptors, 1, 1);
        let sampler_descriptors = gpu::create_sampler_descriptor_heap(device, 8);
        assert!(!sampler_descriptors.is_null());
        gpu::write_sampler_descriptor(sampler_descriptors, 0, &gpu::SamplerDesc::default());

        // Render target placed after the sampled texture.
        let mut color_desc = gpu::TextureDesc {
            usage: gpu::TextureUsage::color_attachment
                | gpu::TextureUsage::transfer_destination
                | gpu::TextureUsage::sampled,
            ..gpu::TextureDesc::default()
        };
        color_desc.extent = gpu::uint32x3::new(64, 64, 1);
        let mut color_size_align = gpu::SizeAlign::default();
        gpu::get_texture_size_align(device, &color_desc, &mut color_size_align);
        let color_texture =
            gpu::create_texture(device, &color_desc, &texture_heap, size_align.size);
        assert!(!color_texture.is_null());
        let color_view = gpu::create_render_view(color_texture, &gpu::RenderViewDesc::default());
        assert!(!color_view.is_null());

        // Timeline, command pool, and a recorded batch.
        let semaphore = gpu::create_timeline_semaphore(device, 0);
        assert!(!semaphore.is_null());
        let pool = gpu::create_command_pool(device, 0);
        assert!(!pool.is_null());

        let mut timestamps = [0u64; 2];
        let commands = gpu::begin_commands(pool);
        assert!(!commands.is_null());
        gpu::set_texture_descriptor_heap(commands, texture_descriptors);
        gpu::set_sampler_descriptor_heap(commands, sampler_descriptors);
        gpu::barrier(
            commands,
            gpu::Stage::none,
            gpu::Access::none,
            gpu::Stage::transfer,
            gpu::Access::transfer_read | gpu::Access::transfer_write,
        );
        gpu::initialize_texture(commands, texture);
        gpu::initialize_texture(commands, color_texture);
        gpu::write_timestamp(commands, timestamps.as_mut_ptr(), gpu::Stage::transfer);

        let source = gpu::GpuRange {
            gpu: heap.range.gpu.cast(),
            size: 256,
        };
        let destination = gpu::GpuRange {
            gpu: heap.range.gpu.add(256).cast(),
            size: 256,
        };
        gpu::copy_memory(commands, source, destination);
        gpu::copy_memory_to_texture(commands, source, texture, &gpu::TextureCopyDesc::default());

        let mut colors = [gpu::ColorAttachment {
            render_view: color_view,
            load: gpu::LoadOp::clear,
            store: gpu::StoreOp::store,
            clear: gpu::ClearColor {
                x: 0.1,
                y: 0.2,
                z: 0.3,
                w: 1.0,
            },
        }];
        gpu::barrier(
            commands,
            gpu::Stage::transfer,
            gpu::Access::transfer_write,
            gpu::Stage::color_output,
            gpu::Access::color_write,
        );
        let rendering = gpu::RenderingDesc {
            colors: gpu::Span::from_slice(&mut colors),
            ..gpu::RenderingDesc::default()
        };
        gpu::begin_render_pass(commands, &rendering, gpu::RenderingFlags::none);
        gpu::set_viewport(
            commands,
            &gpu::Viewport {
                width: 64.0,
                height: 64.0,
                ..gpu::Viewport::default()
            },
        );
        gpu::set_scissor(
            commands,
            &gpu::Scissor {
                width: 64,
                height: 64,
                ..gpu::Scissor::default()
            },
        );
        gpu::set_depth_stencil(commands, &gpu::DepthStencilState::default());
        gpu::end_render_pass(commands);
        gpu::write_timestamp(
            commands,
            timestamps.as_mut_ptr().add(1),
            gpu::Stage::transfer,
        );
        gpu::end_commands(commands);

        // `acquire` asserts on a headless device: it needs a swapchain.
        let mut submits = [commands];
        let completion = gpu::TimelinePoint {
            semaphore,
            value: 1,
        };
        let submit_desc = gpu::SubmitDesc {
            commands: gpu::Span::from_slice(&mut submits),
            waits: gpu::Span::empty(),
            completion,
        };
        gpu::submit(device, &submit_desc, 0);
        gpu::wait_timeline(completion);
        assert_eq!(gpu::timeline_completed_value(semaphore), 1);
        gpu::wait_idle(device);
        gpu::read_timestamps(pool);
        assert!(timestamps[1] >= timestamps[0]);
        gpu::reset_command_pool(pool);

        assert_eq!(
            gpu::set_swapchain_format(device, gpu::Format::undefined, gpu::ColorSpace::srgb),
            gpu::Error::unsupported
        );

        gpu::destroy_command_pool(pool);
        gpu::destroy_timeline_semaphore(semaphore);
        gpu::destroy_sampler_descriptor_heap(sampler_descriptors);
        gpu::destroy_texture_descriptor_heap(texture_descriptors);
        gpu::destroy_render_view(color_view);
        gpu::destroy_texture(color_texture);
        gpu::destroy_render_view(view);
        gpu::destroy_texture(texture);
        gpu::destroy_texture_heap(&texture_heap);
        gpu::destroy_gpu_heap(&heap);
        gpu::destroy_device(device);
        gpu::set_error_callback(None);
    }
}
