//! Port of upstream `utility/tests/upload_queue_test.cpp`.
//!
//! Runs the ring, tiled texture, reclaim, timestamp, destroy, and submission-handoff scenarios on a
//! headless device, verifying every uploaded byte through a guarded readback heap. Skips when no
//! Vulkan device is available. The compute-dispatch scenario needs a precompiled shader and is
//! reduced to a barrier/plumbing check.

#![allow(clippy::useless_conversion)] // `size_t` is `u64` on this target; see the crate docs.

use no_graphics_api_sys as sys;
use no_graphics_api_utility::{UploadQueue, texture_upload::upload_texture};

const BUFFER_BYTES: usize = 20 * 1024;
const READBACK_BYTES: usize = 128 * 1024;
const GUARD_BYTES: usize = 32;
const TEXTURE_COUNT: usize = 6;
const TIMESTAMP_BATCHES: usize = 4;
const SENTINEL: u8 = 0xa5;
const SENTINEL_U64: u64 = 0xa5a5_a5a5_a5a5_a5a5;

fn align16(value: usize) -> usize {
    (value + 15) & !15
}

#[derive(Clone, Copy)]
struct TextureCase {
    texture: usize,
    mip: u32,
    face: u32,
    bytes: usize,
    offset: usize,
}

struct Fixture {
    device: *mut sys::Device,
    pool: *mut sys::CommandPool,
    ring: UploadQueue,
    completion: sys::TimelinePoint,
    buffer: sys::GpuHeap,
    readback: sys::GpuHeap,
    texture_heap: sys::TextureHeap,
    textures: [*mut sys::Texture; TEXTURE_COUNT],
    attachments: [*mut sys::RenderView; 2],
    descriptions: [sys::TextureDesc; TEXTURE_COUNT],
    cases: Vec<TextureCase>,
    expected: Vec<u8>,
}

unsafe fn create_test_device() -> Option<*mut sys::Device> {
    let description = sys::DeviceDesc {
        desired_queue_count: 2,
        timestamp_query_count: 2,
        ..sys::DeviceDesc::default()
    };
    let mut init = sys::DeviceInit::default();
    unsafe { sys::create_device(&description, &mut init) };
    if init.device.is_null() {
        eprintln!(
            "no compatible Vulkan device, skipping (error: {:?})",
            init.error
        );
        return None;
    }

    Some(init.device)
}

fn transfer_usage() -> sys::TextureUsage {
    sys::TextureUsage::transfer_source | sys::TextureUsage::transfer_destination
}

fn texture_descriptions() -> [sys::TextureDesc; TEXTURE_COUNT] {
    [
        sys::TextureDesc {
            type_: sys::TextureType::three_d,
            extent: sys::uint32x3::new(37, 23, 11),
            format: sys::Format::r8_unorm,
            usage: transfer_usage(),
            ..sys::TextureDesc::default()
        },
        sys::TextureDesc {
            type_: sys::TextureType::two_d_array,
            extent: sys::uint32x3::new(519, 7, 1),
            layer_count: 5,
            format: sys::Format::r8_unorm,
            usage: transfer_usage(),
            ..sys::TextureDesc::default()
        },
        sys::TextureDesc {
            extent: sys::uint32x3::new(137, 9, 1),
            mip_levels: 8,
            format: sys::Format::bc6h_ufloat,
            usage: transfer_usage(),
            ..sys::TextureDesc::default()
        },
        sys::TextureDesc {
            type_: sys::TextureType::cube,
            extent: sys::uint32x3::new(9, 9, 1),
            mip_levels: 4,
            layer_count: 6,
            format: sys::Format::bc6h_ufloat,
            usage: transfer_usage(),
            ..sys::TextureDesc::default()
        },
        sys::TextureDesc {
            extent: sys::uint32x3::new(7, 5, 1),
            format: sys::Format::rgba8_unorm,
            usage: sys::TextureUsage::color_attachment | transfer_usage(),
            ..sys::TextureDesc::default()
        },
        sys::TextureDesc {
            extent: sys::uint32x3::new(7, 5, 1),
            format: sys::Format::d32_float,
            usage: sys::TextureUsage::depth_stencil_attachment | transfer_usage(),
            ..sys::TextureDesc::default()
        },
    ]
}

fn gpu_range_of_heap(heap: &sys::GpuHeap) -> sys::GpuRange {
    sys::GpuRange {
        gpu: heap.range.gpu.cast(),
        size: heap.range.size,
    }
}

unsafe fn initialize(device: *mut sys::Device) -> Option<Fixture> {
    unsafe {
        let mut descriptions = texture_descriptions();
        let caps = &*sys::get_device_caps(device);

        for description in &mut descriptions {
            if (description.format == sys::Format::bc6h_ufloat
                || description.format == sys::Format::bc6h_sfloat)
                && !caps.texture_compression_bc
            {
                description.format = sys::Format::rgba8_unorm;
            }
        }
        for description in &descriptions {
            if !sys::supports_texture_format(device, description.format, description.usage) {
                eprintln!("device does not support the required texture formats, skipping");
                return None;
            }
        }

        let mut fixture = Fixture {
            device,
            pool: sys::create_command_pool(device, 0),
            ring: UploadQueue::new(device, 256, 0, 2)?,
            completion: sys::TimelinePoint {
                semaphore: sys::create_timeline_semaphore(device, 0),
                value: 0,
            },
            buffer: sys::GpuHeap::default(),
            readback: sys::GpuHeap::default(),
            texture_heap: sys::TextureHeap::default(),
            textures: [core::ptr::null_mut(); TEXTURE_COUNT],
            attachments: [core::ptr::null_mut(); 2],
            descriptions,
            cases: Vec::new(),
            expected: vec![SENTINEL; READBACK_BYTES],
        };
        assert!(!fixture.pool.is_null(), "command pool creation failed");
        assert!(
            !fixture.completion.semaphore.is_null(),
            "timeline semaphore creation failed"
        );

        sys::create_gpu_heap(
            device,
            BUFFER_BYTES as u64,
            sys::MemoryType::gpu_only,
            &mut fixture.buffer,
        );
        assert!(
            !fixture.buffer.owner.is_null(),
            "buffer heap creation failed"
        );
        sys::create_gpu_heap(
            device,
            READBACK_BYTES as u64,
            sys::MemoryType::readback,
            &mut fixture.readback,
        );
        assert!(
            !fixture.readback.owner.is_null(),
            "readback heap creation failed"
        );

        let mut heap_bytes = 0u64;
        let mut offsets = [0u64; TEXTURE_COUNT];
        for (index, description) in fixture.descriptions.iter().enumerate() {
            let mut size = sys::SizeAlign::default();
            sys::get_texture_size_align(device, description, &mut size);
            assert!(size.size != 0, "texture size alignment failed");
            offsets[index] =
                (heap_bytes + u64::from(size.align) - 1) & !(u64::from(size.align) - 1);
            heap_bytes = offsets[index] + u64::from(size.size);
        }
        sys::create_texture_heap(device, heap_bytes, &mut fixture.texture_heap);
        assert!(
            !fixture.texture_heap.owner.is_null(),
            "texture heap creation failed"
        );

        let initialization = sys::begin_commands(fixture.pool);
        for (index, description) in fixture.descriptions.iter().enumerate() {
            fixture.textures[index] =
                sys::create_texture(device, description, &fixture.texture_heap, offsets[index]);
            assert!(
                !fixture.textures[index].is_null(),
                "texture creation failed"
            );
            sys::initialize_texture(initialization, fixture.textures[index]);
        }
        sys::end_commands(initialization);
        fixture.completion.value += 1;
        let mut commands = [initialization];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: sys::Span::empty(),
            completion: fixture.completion,
        };
        sys::submit(device, &submit, 0);
        sys::wait_timeline(fixture.completion);
        sys::reset_command_pool(fixture.pool);

        for index in 0..2 {
            fixture.attachments[index] = sys::create_render_view(
                fixture.textures[index + 4],
                &sys::RenderViewDesc::default(),
            );
            assert!(
                !fixture.attachments[index].is_null(),
                "render view creation failed"
            );
        }

        let mut output = BUFFER_BYTES + GUARD_BYTES * 2;
        for (index, description) in fixture.descriptions.iter().enumerate() {
            let mut format = sys::TextureFormatInfo::default();
            sys::get_texture_format_info(description.format, &mut format);
            let faces = if description.type_ == sys::TextureType::cube {
                6
            } else {
                1
            };
            for face in 0..faces {
                for mip in 0..description.mip_levels {
                    let width = (description.extent.x >> mip).max(1);
                    let height = (description.extent.y >> mip).max(1);
                    let depth = (description.extent.z >> mip).max(1);
                    let mut bytes = width.div_ceil(format.block_extent.x) as usize
                        * height.div_ceil(format.block_extent.y) as usize
                        * format.bytes_per_block as usize
                        * depth as usize;
                    if description.type_ == sys::TextureType::two_d_array {
                        bytes *= description.layer_count as usize;
                    }
                    output = align16(output);
                    fixture.cases.push(TextureCase {
                        texture: index,
                        mip,
                        face,
                        bytes,
                        offset: output,
                    });
                    for byte in 0..bytes {
                        fixture.expected[output + byte] = (byte * 37
                            + (byte >> 7) * 19
                            + index * 23
                            + face as usize * 53
                            + mip as usize * 11)
                            as u8;
                    }
                    if index == 5 {
                        for pixel in 0..bytes / 4 {
                            let value = (pixel + 1) as f32 / 64.0f32;
                            fixture.expected[output + pixel * 4..output + pixel * 4 + 4]
                                .copy_from_slice(&value.to_le_bytes());
                        }
                    }
                    output += bytes + GUARD_BYTES;
                }
            }
        }
        assert!(
            output < READBACK_BYTES,
            "fixture fits its guarded readback allocation"
        );

        Some(fixture)
    }
}

unsafe fn shutdown(fixture: &mut Fixture) {
    unsafe {
        if fixture.device.is_null() {
            return;
        }

        fixture.ring.wait();
        sys::wait_idle(fixture.device);
        fixture.ring.destroy();
        sys::destroy_command_pool(fixture.pool);
        for attachment in fixture.attachments {
            if !attachment.is_null() {
                sys::destroy_render_view(attachment);
            }
        }
        for texture in fixture.textures {
            if !texture.is_null() {
                sys::destroy_texture(texture);
            }
        }
        if !fixture.texture_heap.owner.is_null() {
            sys::destroy_texture_heap(&fixture.texture_heap);
        }
        if !fixture.buffer.owner.is_null() {
            sys::destroy_gpu_heap(&fixture.buffer);
        }
        if !fixture.readback.owner.is_null() {
            sys::destroy_gpu_heap(&fixture.readback);
        }
        if !fixture.completion.semaphore.is_null() {
            sys::destroy_timeline_semaphore(fixture.completion.semaphore);
        }
        sys::destroy_device(fixture.device);
        fixture.device = core::ptr::null_mut();
    }
}

unsafe fn read_results(fixture: &mut Fixture) {
    unsafe {
        let uploaded = fixture.ring.flush();
        fixture.ring.wait();

        core::ptr::write_bytes(fixture.readback.range.cpu, SENTINEL, READBACK_BYTES);
        let commands = sys::begin_commands(fixture.pool);
        sys::copy_memory(
            commands,
            gpu_range_of_heap(&fixture.buffer),
            sys::GpuRange {
                gpu: (fixture.readback.range.gpu as usize + GUARD_BYTES) as *mut core::ffi::c_void,
                size: BUFFER_BYTES as _,
            },
        );
        for case in fixture.cases.clone() {
            let copy = sys::TextureCopyDesc {
                mip_level: case.mip,
                base_slice: case.face,
                slice_count: if case.texture == 3 { 1 } else { 0 },
                ..sys::TextureCopyDesc::default()
            };
            sys::copy_texture_to_memory(
                commands,
                fixture.textures[case.texture],
                sys::GpuRange {
                    gpu: (fixture.readback.range.gpu as usize + case.offset)
                        as *mut core::ffi::c_void,
                    size: case.bytes as _,
                },
                &copy,
            );
        }
        sys::barrier(
            commands,
            sys::Stage::transfer,
            sys::Access::transfer_write,
            sys::Stage::host,
            sys::Access::host_read,
        );
        sys::end_commands(commands);

        fixture.completion.value += 1;
        let mut commands = [commands];
        let mut waits = [uploaded];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: if uploaded.semaphore.is_null() {
                sys::Span::empty()
            } else {
                sys::Span::from_slice(&mut waits)
            },
            completion: fixture.completion,
        };
        sys::submit(fixture.device, &submit, 0);
        sys::wait_timeline(fixture.completion);
        sys::reset_command_pool(fixture.pool);

        let readback = core::slice::from_raw_parts(fixture.readback.range.cpu, READBACK_BYTES);
        assert_eq!(
            readback,
            &fixture.expected[..],
            "uploaded bytes and readback sentinels mismatch"
        );
    }
}

unsafe fn order_queue(fixture: &mut Fixture, queue_index: u32) {
    unsafe {
        let pool = sys::create_command_pool(fixture.device, queue_index);
        assert!(!pool.is_null(), "queue command pool creation failed");
        let commands = sys::begin_commands(pool);
        sys::end_commands(commands);

        let previous = fixture.completion;
        fixture.completion.value += 1;
        let mut commands = [commands];
        let mut waits = [previous];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: sys::Span::from_slice(&mut waits),
            completion: fixture.completion,
        };
        sys::submit(fixture.device, &submit, queue_index);
        sys::wait_timeline(fixture.completion);
        sys::destroy_command_pool(pool);
    }
}

fn case_copy(case: &TextureCase) -> sys::TextureCopyDesc {
    sys::TextureCopyDesc {
        mip_level: case.mip,
        base_slice: case.face,
        slice_count: if case.texture == 3 { 1 } else { 0 },
        ..sys::TextureCopyDesc::default()
    }
}

unsafe fn run_small_ring(fixture: &mut Fixture) {
    unsafe {
        let buffer = gpu_range_of_heap(&fixture.buffer);
        let source = fixture.expected[GUARD_BYTES..GUARD_BYTES + BUFFER_BYTES].to_vec();
        fixture.ring.upload_buffer(buffer, &source);
        fixture.ring.flush();

        let mut bytes = [0u8; 4096];
        for iteration in 0..64usize {
            for (index, byte) in bytes.iter_mut().take(128).enumerate() {
                *byte = (iteration * 31 + index * 7) as u8;
            }
            fixture.expected
                [GUARD_BYTES + 4096 + iteration * 128..GUARD_BYTES + 4096 + iteration * 128 + 128]
                .copy_from_slice(&bytes[..128]);
            let destination = sys::GpuRange {
                gpu: (fixture.buffer.range.gpu as usize + 4096 + iteration * 128)
                    as *mut core::ffi::c_void,
                size: 128,
            };
            fixture.ring.upload_buffer(destination, &bytes[..128]);
        }

        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index * 43 + (index >> 8) * 17) as u8;
        }
        fixture.expected[GUARD_BYTES + 512..GUARD_BYTES + 768].copy_from_slice(&bytes[..256]);
        let destination = sys::GpuRange {
            gpu: (fixture.buffer.range.gpu as usize + 512) as *mut core::ffi::c_void,
            size: 256,
        };
        fixture.ring.upload_buffer(destination, &bytes[..256]);

        fixture.expected[GUARD_BYTES + 12288..GUARD_BYTES + 12288 + bytes.len()]
            .copy_from_slice(&bytes);
        let destination = sys::GpuRange {
            gpu: (fixture.buffer.range.gpu as usize + 12288) as *mut core::ffi::c_void,
            size: bytes.len() as _,
        };
        fixture.ring.upload_buffer(destination, &bytes);
        fixture.ring.flush();

        for iteration in 0..64usize {
            let value = 0x91a6_0f23u32.wrapping_mul(iteration as u32 + 1);
            let value_bytes = value.to_ne_bytes();
            fixture.expected
                [GUARD_BYTES + 256 + iteration * 4..GUARD_BYTES + 256 + iteration * 4 + 4]
                .copy_from_slice(&value_bytes);
            let destination = sys::GpuRange {
                gpu: (fixture.buffer.range.gpu as usize + 256 + iteration * 4)
                    as *mut core::ffi::c_void,
                size: 4,
            };
            fixture.ring.upload_buffer(destination, &value_bytes);
            fixture.ring.flush();
            assert!(
                fixture.ring.stats().pending_batches <= 2,
                "default batch limit bounds pending batches"
            );
        }

        for case in fixture.cases.clone() {
            let description = fixture.descriptions[case.texture];
            let copy = case_copy(&case);
            upload_texture(
                &mut fixture.ring,
                fixture.textures[case.texture],
                &description,
                &fixture.expected[case.offset..case.offset + case.bytes],
                &copy,
            );
        }
        fixture.ring.flush();

        for (index, byte) in bytes.iter_mut().enumerate().take(252) {
            *byte = (173 + index * 13) as u8;
        }
        let copy = sys::TextureCopyDesc {
            offset: sys::uint32x3::new(3, 5, 2),
            extent: sys::uint32x3::new(7, 9, 4),
            ..sys::TextureCopyDesc::default()
        };
        upload_texture(
            &mut fixture.ring,
            fixture.textures[0],
            &fixture.descriptions[0],
            &bytes[..252],
            &copy,
        );
        let base = fixture.cases[0].offset;
        for z in 0..4usize {
            for y in 0..9usize {
                let destination = base + ((z + 2) * 23 + y + 5) * 37 + 3;
                fixture.expected[destination..destination + 7]
                    .copy_from_slice(&bytes[(z * 9 + y) * 7..(z * 9 + y) * 7 + 7]);
            }
        }

        let copy = sys::TextureCopyDesc {
            base_slice: 2,
            slice_count: 2,
            offset: sys::uint32x3::new(5, 1, 0),
            extent: sys::uint32x3::new(3, 4, 1),
            ..sys::TextureCopyDesc::default()
        };
        upload_texture(
            &mut fixture.ring,
            fixture.textures[1],
            &fixture.descriptions[1],
            &bytes[..24],
            &copy,
        );
        let base = fixture.cases[1].offset;
        for z in 0..2usize {
            for y in 0..4usize {
                let destination = base + ((z + 2) * 7 + y + 1) * 519 + 5;
                fixture.expected[destination..destination + 3]
                    .copy_from_slice(&bytes[(z * 4 + y) * 3..(z * 4 + y) * 3 + 3]);
            }
        }

        read_results(fixture);

        let stats = fixture.ring.stats();
        assert_eq!(
            stats.capacity, 256,
            "oversized uploads retain the fixed capacity"
        );
        assert!(
            stats.peak_bytes <= 256,
            "oversized uploads retain the fixed capacity"
        );
        assert_eq!(stats.pending_operations, 0, "wait drains commands");
        assert_eq!(stats.pending_batches, 0, "wait retires every batch");
        assert_eq!(stats.bytes_in_use, 0, "wait retires every staged byte");
        eprintln!(
            "upload queue: {} submissions, {} waits, peak {} bytes",
            stats.submissions, stats.waits, stats.peak_bytes
        );
    }
}

unsafe fn run_pitched_texture(fixture: &mut Fixture) {
    unsafe {
        let mut bytes = [0xd7u8; 160];
        for row in 0..5usize {
            for column in 0..28usize {
                bytes[row * 32 + column] = (row * 53 + column * 19) as u8;
            }
        }
        for case in fixture.cases.clone() {
            if case.texture == 4 {
                for row in 0..5usize {
                    let destination = case.offset + row * 28;
                    fixture.expected[destination..destination + 28]
                        .copy_from_slice(&bytes[row * 32..row * 32 + 28]);
                }
            }
        }

        let copy = sys::TextureCopyDesc {
            row_pitch_bytes: 32,
            slice_pitch_bytes: 160,
            ..sys::TextureCopyDesc::default()
        };
        fixture
            .ring
            .upload_texture(fixture.textures[4], &bytes, &copy);
        read_results(fixture);
    }
}

unsafe fn run_attachment_consumers(fixture: &mut Fixture) {
    unsafe {
        for case in fixture.cases.clone() {
            if case.texture < 4 {
                continue;
            }

            if case.texture == 4 {
                for byte in 0..case.bytes {
                    fixture.expected[case.offset + byte] ^= 0x5a;
                }
            } else {
                for pixel in 0..case.bytes / 4 {
                    let depth = (64 - pixel) as f32 / 128.0f32;
                    fixture.expected[case.offset + pixel * 4..case.offset + pixel * 4 + 4]
                        .copy_from_slice(&depth.to_le_bytes());
                }
            }

            let description = fixture.descriptions[case.texture];
            let copy = case_copy(&case);
            upload_texture(
                &mut fixture.ring,
                fixture.textures[case.texture],
                &description,
                &fixture.expected[case.offset..case.offset + case.bytes],
                &copy,
            );
        }
        fixture.ring.flush();

        let mut colors = [sys::ColorAttachment {
            render_view: fixture.attachments[0],
            ..sys::ColorAttachment::default()
        }];
        let rendering = sys::RenderingDesc {
            colors: sys::Span::from_slice(&mut colors),
            depth: sys::DepthAttachment {
                render_view: fixture.attachments[1],
                ..sys::DepthAttachment::default()
            },
            ..sys::RenderingDesc::default()
        };

        let commands = sys::begin_commands(fixture.pool);
        sys::begin_render_pass(commands, &rendering, sys::RenderingFlags::none);
        sys::end_render_pass(commands);
        sys::barrier(
            commands,
            sys::Stage::color_output | sys::Stage::depth_stencil_tests,
            sys::Access::color_write | sys::Access::depth_stencil_write,
            sys::Stage::transfer,
            sys::Access::transfer_read,
        );
        sys::end_commands(commands);

        fixture.completion.value += 1;
        let mut commands = [commands];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: sys::Span::empty(),
            completion: fixture.completion,
        };
        sys::submit(fixture.device, &submit, 0);
        sys::wait_timeline(fixture.completion);
        sys::reset_command_pool(fixture.pool);

        read_results(fixture);
    }
}

unsafe fn run_reclaim_before_flush(fixture: &mut Fixture) {
    unsafe {
        fixture.ring = UploadQueue::new(fixture.device, 256, 0, 2).expect("upload queue");
        let mut bytes = [0u8; 320];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index * 53 + 17) as u8;
        }
        fixture.expected[GUARD_BYTES..GUARD_BYTES + bytes.len()].copy_from_slice(&bytes);

        let buffer = gpu_range_of_heap(&fixture.buffer);
        fixture.ring.upload_buffer(buffer, &bytes[..128]);
        fixture.ring.flush();
        sys::wait_idle(fixture.device);

        let destination = sys::GpuRange {
            gpu: (fixture.buffer.range.gpu as usize + 128) as *mut core::ffi::c_void,
            size: 64,
        };
        fixture.ring.upload_buffer(destination, &bytes[128..192]);
        assert_eq!(
            fixture.ring.stats().bytes_in_use,
            192,
            "pending bytes are tracked"
        );
        assert_eq!(
            fixture.ring.stats().pending_batches,
            1,
            "completed retirement remains uncollected"
        );

        let submissions = fixture.ring.stats().submissions;
        let destination = sys::GpuRange {
            gpu: (fixture.buffer.range.gpu as usize + 192) as *mut core::ffi::c_void,
            size: 128,
        };
        fixture.ring.upload_buffer(destination, &bytes[192..320]);
        assert_eq!(
            fixture.ring.stats().submissions,
            submissions,
            "completed staging space is reclaimed without flushing"
        );
        assert_eq!(
            fixture.ring.stats().pending_operations,
            2,
            "pending copies are preserved"
        );

        read_results(fixture);
    }
}

unsafe fn run_empty_batches_and_destroy(fixture: &mut Fixture) {
    unsafe {
        fixture.ring = UploadQueue::new(fixture.device, 256, 0, 2).expect("upload queue");

        let guard_words = GUARD_BYTES / 8;
        let mut storage = vec![SENTINEL_U64; TIMESTAMP_BATCHES * 2 + guard_words * 2];
        let timestamps = storage.as_mut_ptr().add(guard_words);

        let mut completion = sys::TimelinePoint::default();
        for index in 0..TIMESTAMP_BATCHES {
            let destination = timestamps.add(index * 2);
            fixture.ring.write_timestamp(destination);
            fixture.ring.write_timestamp(destination.add(1));
            completion = fixture.ring.flush();
        }
        sys::wait_timeline(completion);
        assert_eq!(
            *timestamps.add((TIMESTAMP_BATCHES - 1) * 2),
            SENTINEL_U64,
            "completion leaves timestamp destinations unchanged until retrieval"
        );

        let waits = fixture.ring.stats().waits;
        fixture.ring.reclaim();
        assert_eq!(
            fixture.ring.stats().waits,
            waits,
            "completed timestamp batches are reclaimed without another wait"
        );

        let stats = fixture.ring.stats();
        assert_eq!(
            stats.submissions, TIMESTAMP_BATCHES as u64,
            "timestamp-only batches submit"
        );
        assert_eq!(
            stats.bytes_in_use, 0,
            "timestamp-only batches reserve no staging bytes"
        );
        assert_eq!(stats.pending_batches, 0, "timestamp-only batches retire");
        for index in 0..TIMESTAMP_BATCHES {
            let first = *timestamps.add(index * 2);
            let second = *timestamps.add(index * 2 + 1);
            assert!(
                first != SENTINEL_U64 && second != SENTINEL_U64,
                "timestamps were retrieved"
            );
            assert!(first <= second, "timestamps are ordered");
        }
        for (index, word) in storage.iter().enumerate() {
            if index < guard_words || index >= TIMESTAMP_BATCHES * 2 + guard_words {
                assert_eq!(*word, SENTINEL_U64, "timestamp guards remain unchanged");
            }
        }

        let mut bytes = [0u8; 64];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index * 29 + 67) as u8;
        }
        fixture.expected[GUARD_BYTES..GUARD_BYTES + bytes.len()].copy_from_slice(&bytes);

        let mut destruction = [SENTINEL_U64; 2];
        fixture.ring.write_timestamp(destruction.as_mut_ptr());
        let buffer = gpu_range_of_heap(&fixture.buffer);
        fixture.ring.upload_buffer(buffer, &bytes);
        fixture
            .ring
            .write_timestamp(destruction.as_mut_ptr().add(1));
        assert_eq!(
            fixture.ring.stats().pending_operations,
            1,
            "destruction starts with an unsubmitted upload"
        );

        fixture.ring.destroy();
        assert!(
            destruction[0] != SENTINEL_U64
                && destruction[1] != SENTINEL_U64
                && destruction[0] <= destruction[1],
            "destruction retrieves timestamps before destroying command pools"
        );
        let stats = fixture.ring.stats();
        assert_eq!(stats.capacity, 0, "destruction resets the ring");
        assert_eq!(
            stats.pending_operations, 0,
            "destruction resets pending operations"
        );
        assert_eq!(
            stats.pending_batches, 0,
            "destruction resets pending batches"
        );

        read_results(fixture);
    }
}

#[allow(unused_assignments)]
unsafe fn run_move_ownership(fixture: &mut Fixture) {
    unsafe {
        fixture.ring.wait();
        let mut bytes = [0u8; 64];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index * 41 + 93) as u8;
        }
        fixture.expected[GUARD_BYTES + 512..GUARD_BYTES + 512 + bytes.len()]
            .copy_from_slice(&bytes);

        {
            let mut original = UploadQueue::new(fixture.device, 256, 0, 3).expect("upload queue");
            let destination = sys::GpuRange {
                gpu: (fixture.buffer.range.gpu as usize + 512) as *mut core::ffi::c_void,
                size: bytes.len() as _,
            };
            original.upload_buffer(destination, &bytes);

            let moved = original;
            assert_eq!(
                moved.stats().pending_operations,
                1,
                "moving transfers unsubmitted copies"
            );

            let mut replacement =
                UploadQueue::new(fixture.device, 128, 0, 1).expect("replacement queue");
            replacement = moved;
            assert_eq!(
                replacement.stats().capacity,
                256,
                "assignment takes the source ring"
            );
            assert_eq!(
                replacement.stats().pending_operations,
                1,
                "assignment preserves pending copies"
            );

            fixture.ring = replacement;
        }

        read_results(fixture);
    }
}

unsafe fn run_operation_limit(fixture: &mut Fixture) {
    unsafe {
        fixture.ring = UploadQueue::new(fixture.device, 128 * 1024, 0, 2).expect("upload queue");
        let submissions = fixture.ring.stats().submissions;
        for index in 0..UploadQueue::OPERATION_LIMIT + 1 {
            let value = 0xde81_7f03u32.wrapping_mul(index + 1);
            let bytes = value.to_ne_bytes();
            let offset = GUARD_BYTES + 32 + index as usize * 4;
            fixture.expected[offset..offset + 4].copy_from_slice(&bytes);
            let destination = sys::GpuRange {
                gpu: (fixture.buffer.range.gpu as usize + 32 + index as usize * 4)
                    as *mut core::ffi::c_void,
                size: 4,
            };
            fixture.ring.upload_buffer(destination, &bytes);
        }
        assert_eq!(
            fixture.ring.stats().submissions,
            submissions + 1,
            "copy count flushes before staging exceeds the operation limit"
        );
        assert_eq!(
            fixture.ring.stats().pending_operations,
            1,
            "the flush keeps the newest copy pending"
        );

        read_results(fixture);
        assert_eq!(
            fixture.ring.stats().capacity,
            128 * 1024,
            "operation pressure does not grow the ring"
        );
    }
}

unsafe fn run_submission_handoff(fixture: &mut Fixture) {
    unsafe {
        fixture.ring = UploadQueue::new(fixture.device, 256, 0, 2).expect("upload queue");
        let handoffs = std::rc::Rc::new(core::cell::Cell::new(0u32));
        let counter = std::rc::Rc::clone(&handoffs);
        fixture.ring.set_submit_callback(Some(Box::new(
            move |device, commands, completion, queue_index| {
                counter.set(counter.get() + 1);
                let mut commands = [commands];
                let desc = sys::SubmitDesc {
                    commands: sys::Span::from_slice(&mut commands),
                    waits: sys::Span::empty(),
                    completion,
                };
                sys::submit(device, &desc, queue_index);
            },
        )));

        let mut bytes = [0u8; 1024];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index * 17) as u8;
        }
        let buffer = gpu_range_of_heap(&fixture.buffer);
        fixture.ring.upload_buffer(buffer, &bytes);
        fixture.expected[GUARD_BYTES..GUARD_BYTES + bytes.len()].copy_from_slice(&bytes);
        fixture.ring.wait();

        assert_eq!(handoffs.get(), 4, "handoff receives the automatic flushes");
        assert_eq!(
            fixture.ring.stats().submissions,
            4,
            "handoff replaces automatic submission"
        );

        fixture.ring.set_submit_callback(None);
        bytes[0] = 123;
        let destination = sys::GpuRange {
            gpu: fixture.buffer.range.gpu.cast(),
            size: 4,
        };
        fixture.ring.upload_buffer(destination, &bytes[..4]);
        fixture.expected[GUARD_BYTES] = 123;
        fixture.ring.wait();
        assert_eq!(
            handoffs.get(),
            4,
            "clearing the handoff restores ordinary submission"
        );
        assert_eq!(
            fixture.ring.stats().submissions,
            5,
            "ordinary submission resumes"
        );

        read_results(fixture);
    }
}

unsafe fn run_non_power_of_two(fixture: &mut Fixture, max_pending_batches: u32) {
    unsafe {
        fixture.ring =
            UploadQueue::new(fixture.device, 272, 0, max_pending_batches).expect("upload queue");
        let mut bytes = [0u8; 272];
        let mut offset = 1024usize;
        for iteration in 0..8usize {
            for size in [272usize, 4, 20] {
                for (index, byte) in bytes.iter_mut().take(size).enumerate() {
                    *byte = (iteration * 71 + size * 3 + index * 19) as u8;
                }
                fixture.expected[GUARD_BYTES + offset..GUARD_BYTES + offset + size]
                    .copy_from_slice(&bytes[..size]);
                let destination = sys::GpuRange {
                    gpu: (fixture.buffer.range.gpu as usize + offset) as *mut core::ffi::c_void,
                    size: size as _,
                };
                fixture.ring.upload_buffer(destination, &bytes[..size]);
                offset += size;
            }
        }

        read_results(fixture);
        let stats = fixture.ring.stats();
        assert_eq!(
            stats.capacity, 272,
            "ring keeps its non-power-of-two capacity"
        );
        assert_eq!(
            stats.peak_bytes, 272,
            "exact-capacity uploads wrap within the ring"
        );
        assert_eq!(stats.bytes_in_use, 0, "the ring drains");
    }
}

unsafe fn run_max_pending_batches(fixture: &mut Fixture, max_pending_batches: u32) {
    unsafe {
        if (*sys::get_device_caps(fixture.device)).queue_count < 2 {
            return;
        }

        fixture.ring =
            UploadQueue::new(fixture.device, 1024, 0, max_pending_batches).expect("upload queue");
        let gate = sys::create_timeline_semaphore(fixture.device, 0);
        assert!(!gate.is_null(), "retirement gate initializes");
        let release_pool = sys::create_command_pool(fixture.device, 0);
        assert!(
            !release_pool.is_null(),
            "retirement release pool initializes"
        );

        let blocked = sys::begin_commands(fixture.pool);
        sys::end_commands(blocked);
        fixture.completion.value += 1;
        let mut blocked = [blocked];
        let mut waits = [sys::TimelinePoint {
            semaphore: gate,
            value: 1,
        }];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut blocked),
            waits: sys::Span::from_slice(&mut waits),
            completion: fixture.completion,
        };
        sys::submit(fixture.device, &submit, 0);

        for index in 0..max_pending_batches as usize {
            let value = 0xcafe_0000u32 + index as u32;
            let bytes = value.to_ne_bytes();
            fixture.expected[GUARD_BYTES + index * 4..GUARD_BYTES + index * 4 + 4]
                .copy_from_slice(&bytes);
            let destination = sys::GpuRange {
                gpu: (fixture.buffer.range.gpu as usize + index * 4) as *mut core::ffi::c_void,
                size: 4,
            };
            fixture.ring.upload_buffer(destination, &bytes);
            fixture.ring.flush();
        }
        assert_eq!(
            fixture.ring.stats().pending_batches,
            max_pending_batches,
            "batches stay in flight while gated"
        );
        assert_eq!(fixture.ring.stats().waits, 0, "gated batches do not wait");

        let placeholder = UploadQueue::new(fixture.device, 16, 0, 1).expect("placeholder queue");
        let ring = core::mem::replace(&mut fixture.ring, placeholder);
        assert_eq!(
            ring.stats().pending_batches,
            max_pending_batches,
            "moving preserves submitted batches"
        );
        let moved = ring;
        fixture.ring = moved;

        let release = sys::begin_commands(release_pool);
        sys::end_commands(release);
        let mut release = [release];
        let release_submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut release),
            waits: sys::Span::empty(),
            completion: sys::TimelinePoint {
                semaphore: gate,
                value: 1,
            },
        };
        sys::submit(
            fixture.device,
            &release_submit,
            1.min((*sys::get_device_caps(fixture.device)).queue_count - 1),
        );

        let final_value: u32 = 0x1973_abcd;
        let bytes = final_value.to_ne_bytes();
        let destination = sys::GpuRange {
            gpu: (fixture.buffer.range.gpu as usize + 256) as *mut core::ffi::c_void,
            size: 4,
        };
        fixture.ring.upload_buffer(destination, &bytes);
        assert!(
            fixture.ring.stats().pending_batches < max_pending_batches,
            "an additional batch reuses a completed slot"
        );
        assert_eq!(
            fixture.ring.stats().pending_operations,
            1,
            "the new batch stays open"
        );
        fixture.expected[GUARD_BYTES + 256..GUARD_BYTES + 260].copy_from_slice(&bytes);

        fixture.ring.wait();
        sys::wait_timeline(fixture.completion);
        sys::wait_timeline(sys::TimelinePoint {
            semaphore: gate,
            value: 1,
        });
        sys::reset_command_pool(fixture.pool);
        sys::destroy_command_pool(release_pool);
        sys::destroy_timeline_semaphore(gate);

        read_results(fixture);
    }
}

unsafe fn run_multiple_queues(fixture: &mut Fixture, queue_index: u32) {
    unsafe {
        let caps = *sys::get_device_caps(fixture.device);
        if caps.queue_count < queue_index + 1 {
            return;
        }

        fixture.ring.destroy();
        let mut ring = UploadQueue::new(fixture.device, 256, queue_index, 3).expect("upload queue");
        order_queue(fixture, queue_index);
        let mut pending = ring.flush();
        pending.value += 1;

        let mut values = [0u32; 64];
        for (index, value) in values.iter_mut().enumerate() {
            *value = (index as u32).wrapping_mul(0x91a6_0723).wrapping_add(11);
        }
        let mut bytes = [0u8; 256];
        for (index, slot) in bytes.chunks_exact_mut(4).enumerate() {
            slot.copy_from_slice(&values[index].to_ne_bytes());
        }
        let destination = gpu_range_of_heap(&fixture.buffer);
        ring.upload_buffer(destination, &bytes);
        fixture.expected[GUARD_BYTES..GUARD_BYTES + bytes.len()].copy_from_slice(&bytes);

        let moved = ring;
        fixture.ring = moved;
        assert_eq!(
            fixture.ring.stats().pending_operations,
            1,
            "moves transfer a nonzero-queue uploader"
        );

        core::ptr::write_bytes(
            fixture.readback.range.cpu,
            SENTINEL,
            bytes.len() + GUARD_BYTES * 2,
        );
        let consumer = sys::begin_commands(fixture.pool);
        let source = sys::GpuRange {
            gpu: destination.gpu,
            size: bytes.len() as _,
        };
        sys::copy_memory(
            consumer,
            source,
            sys::GpuRange {
                gpu: (fixture.readback.range.gpu as usize + GUARD_BYTES) as *mut core::ffi::c_void,
                size: bytes.len() as _,
            },
        );
        sys::barrier(
            consumer,
            sys::Stage::transfer,
            sys::Access::transfer_write,
            sys::Stage::host,
            sys::Access::host_read,
        );
        sys::end_commands(consumer);

        fixture.completion.value += 1;
        let mut consumer = [consumer];
        let mut waits = [pending];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut consumer),
            waits: sys::Span::from_slice(&mut waits),
            completion: fixture.completion,
        };
        sys::submit(fixture.device, &submit, 0);

        let uploaded = fixture.ring.flush();
        let empty_flush = fixture.ring.flush();
        assert_eq!(
            uploaded.semaphore, pending.semaphore,
            "flush returns the latest completion"
        );
        assert_eq!(
            uploaded.value, pending.value,
            "flush returns the latest completion value"
        );
        assert_eq!(
            empty_flush.semaphore, uploaded.semaphore,
            "an empty flush repeats the completion"
        );
        assert_eq!(
            empty_flush.value, uploaded.value,
            "an empty flush repeats the completion value"
        );

        sys::wait_timeline(fixture.completion);
        sys::reset_command_pool(fixture.pool);

        let readback = core::slice::from_raw_parts(
            fixture.readback.range.cpu,
            GUARD_BYTES + bytes.len() + GUARD_BYTES,
        );
        assert_eq!(
            &readback[GUARD_BYTES..GUARD_BYTES + bytes.len()],
            &bytes,
            "cross-queue dependency is satisfied"
        );
        assert!(
            readback[..GUARD_BYTES].iter().all(|byte| *byte == SENTINEL),
            "cross-queue guards remain unchanged"
        );
        assert!(
            readback[GUARD_BYTES + bytes.len()..]
                .iter()
                .all(|byte| *byte == SENTINEL),
            "cross-queue guards remain unchanged"
        );

        fixture.ring.wait();
    }
}

unsafe fn run_compute_plumbing(fixture: &mut Fixture) {
    unsafe {
        fixture.ring = UploadQueue::new(fixture.device, 288, 0, 2).expect("upload queue");
        let mut calls = 0u32;
        let submissions = fixture.ring.stats().submissions;
        fixture.ring.upload_with_compute(128, |frame| {
            calls += 1;
            assert_eq!(
                frame.staging.size, 128,
                "callback receives the requested staging range"
            );
            assert_eq!(
                frame.staging.cpu as usize % 16,
                0,
                "staging CPU address is aligned"
            );
            assert_eq!(
                frame.staging.gpu as usize % 16,
                0,
                "staging GPU address is aligned"
            );
            assert!(
                !frame.commands.is_null(),
                "callback receives a recording command buffer"
            );
            core::ptr::write_bytes(frame.staging.cpu, 0x3c, 128);
        });
        assert_eq!(
            calls, 1,
            "the compute callback runs synchronously exactly once"
        );
        assert_eq!(
            fixture.ring.stats().pending_operations,
            1,
            "the callback counts as one operation"
        );

        fixture.ring.wait();
        assert_eq!(
            fixture.ring.stats().submissions,
            submissions + 1,
            "the compute batch is submitted"
        );
        assert_eq!(
            fixture.ring.stats().bytes_in_use,
            0,
            "the compute batch drains"
        );
    }
}

#[test]
fn upload_queue_conformance() {
    unsafe {
        let Some(device) = create_test_device() else {
            return;
        };
        let Some(mut fixture) = initialize(device) else {
            sys::destroy_device(device);
            return;
        };
        let caps = *sys::get_device_caps(device);
        eprintln!(
            "upload queue conformance: {} queues ({} general, {} compute, {} copy)",
            caps.queue_count,
            caps.general_queue_count,
            caps.compute_queue_count,
            caps.copy_queue_count
        );

        run_small_ring(&mut fixture);
        run_pitched_texture(&mut fixture);
        run_attachment_consumers(&mut fixture);
        run_reclaim_before_flush(&mut fixture);
        run_empty_batches_and_destroy(&mut fixture);
        run_move_ownership(&mut fixture);
        run_operation_limit(&mut fixture);
        run_submission_handoff(&mut fixture);
        run_compute_plumbing(&mut fixture);
        for max_pending_batches in [1, 2, 3] {
            run_non_power_of_two(&mut fixture, max_pending_batches);
            run_max_pending_batches(&mut fixture, max_pending_batches);
        }
        run_multiple_queues(&mut fixture, 1);

        shutdown(&mut fixture);
    }
}

/// Uploads a volume texture whose rows and columns fit one tile but whose slices do not, exercising
/// the contiguous fast path with a nonzero slice offset.
#[test]
fn tiled_upload_splits_along_slices() {
    unsafe {
        let Some(device) = create_test_device() else {
            return;
        };

        let description = sys::TextureDesc {
            type_: sys::TextureType::three_d,
            extent: sys::uint32x3::new(16, 8, 4),
            format: sys::Format::r8_unorm,
            usage: transfer_usage(),
            ..sys::TextureDesc::default()
        };
        let mut size = sys::SizeAlign::default();
        sys::get_texture_size_align(device, &description, &mut size);
        assert!(size.size != 0);

        let mut texture_heap = sys::TextureHeap::default();
        sys::create_texture_heap(device, u64::from(size.size), &mut texture_heap);
        assert!(!texture_heap.owner.is_null());
        let texture = sys::create_texture(device, &description, &texture_heap, 0);
        assert!(!texture.is_null());

        let pool = sys::create_command_pool(device, 0);
        let semaphore = sys::create_timeline_semaphore(device, 0);
        assert!(!pool.is_null() && !semaphore.is_null());
        let mut completion = sys::TimelinePoint {
            semaphore,
            value: 1,
        };
        let commands = sys::begin_commands(pool);
        sys::initialize_texture(commands, texture);
        sys::end_commands(commands);
        let mut commands = [commands];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: sys::Span::empty(),
            completion,
        };
        sys::submit(device, &submit, 0);
        sys::wait_timeline(completion);
        sys::reset_command_pool(pool);

        let source: Vec<u8> = (0..16 * 8 * 4).map(|index| (index * 7 + 3) as u8).collect();
        let mut queue = UploadQueue::new(device, 256, 0, 2).expect("upload queue");
        upload_texture(
            &mut queue,
            texture,
            &description,
            &source,
            &sys::TextureCopyDesc::default(),
        );
        let uploaded = queue.flush();
        queue.wait();

        let mut readback = sys::GpuHeap::default();
        sys::create_gpu_heap(
            device,
            source.len() as u64,
            sys::MemoryType::readback,
            &mut readback,
        );
        assert!(!readback.owner.is_null());
        let commands = sys::begin_commands(pool);
        sys::copy_texture_to_memory(
            commands,
            texture,
            sys::GpuRange {
                gpu: readback.range.gpu.cast(),
                size: source.len() as _,
            },
            &sys::TextureCopyDesc::default(),
        );
        sys::barrier(
            commands,
            sys::Stage::transfer,
            sys::Access::transfer_write,
            sys::Stage::host,
            sys::Access::host_read,
        );
        sys::end_commands(commands);
        completion.value += 1;
        let mut commands = [commands];
        let mut waits = [uploaded];
        let submit = sys::SubmitDesc {
            commands: sys::Span::from_slice(&mut commands),
            waits: sys::Span::from_slice(&mut waits),
            completion,
        };
        sys::submit(device, &submit, 0);
        sys::wait_timeline(completion);
        sys::reset_command_pool(pool);

        let bytes = core::slice::from_raw_parts(readback.range.cpu, source.len());
        assert_eq!(bytes, &source[..], "slice-split upload data mismatch");

        queue.destroy();
        sys::destroy_gpu_heap(&readback);
        sys::destroy_texture(texture);
        sys::destroy_texture_heap(&texture_heap);
        sys::destroy_timeline_semaphore(semaphore);
        sys::destroy_command_pool(pool);
        sys::destroy_device(device);
    }
}
