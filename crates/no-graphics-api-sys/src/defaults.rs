//! `Default` implementations mirroring the C++ member initializers.
//!
//! Any change to a descriptor in the upstream header must be reflected here; the compiler points at
//! every affected impl.

// bindgen does not derive `Default`, so these impls cannot be `#[derive(Default)]` on the types.
#![allow(clippy::derivable_impls)]

use crate::*;

impl Default for uint32x2 {
    fn default() -> Self {
        Self { x: 0, y: 0 }
    }
}

impl Default for uint32x3 {
    fn default() -> Self {
        Self { x: 0, y: 0, z: 0 }
    }
}

impl Default for SizeAlign {
    fn default() -> Self {
        Self { size: 0, align: 0 }
    }
}

impl Default for GpuRange {
    fn default() -> Self {
        Self {
            gpu: core::ptr::null_mut(),
            size: 0,
        }
    }
}

impl Default for GpuHeap {
    fn default() -> Self {
        Self {
            range: GpuCpuRange::default(),
            owner: core::ptr::null(),
        }
    }
}

impl Default for TextureHeap {
    fn default() -> Self {
        Self {
            size: 0,
            owner: core::ptr::null(),
        }
    }
}

impl Default for TimelinePoint {
    fn default() -> Self {
        Self {
            semaphore: core::ptr::null_mut(),
            value: 0,
        }
    }
}

impl Default for SubmitDesc {
    fn default() -> Self {
        Self {
            commands: Span::empty(),
            waits: Span::empty(),
            completion: TimelinePoint::default(),
        }
    }
}

impl Default for TextureFormatInfo {
    fn default() -> Self {
        Self {
            block_extent: uint32x2::default(),
            bytes_per_block: 0,
            depth: false,
            stencil: false,
        }
    }
}

impl Default for DeviceCaps {
    fn default() -> Self {
        Self {
            device_name: core::ptr::null(),
            queue_count: 0,
            general_queue_count: 0,
            compute_queue_count: 0,
            copy_queue_count: 0,
            copy_texture_granularity: uint32x3 { x: 1, y: 1, z: 1 },
            max_push_data_size: 0,
            texture_heap_alignment: 0,
            timestamp_period_ns: 0.0,
            sub_texel_precision_bits: 0,
            texture_compression_bc: false,
            texture_compression_astc: false,
            storage_input_output16: false,
            indirect_mesh_draw: false,
        }
    }
}

impl Default for DeviceDesc {
    fn default() -> Self {
        Self {
            window: core::ptr::null_mut(),
            swapchain_format: Format::undefined,
            swapchain_color_space: ColorSpace::srgb,
            desired_swapchain_image_count: 2,
            steam_overlay_compatibility: false,
            desired_queue_count: 1,
            desired_compute_queue_count: 0,
            desired_copy_queue_count: 0,
            timestamp_query_count: 256,
        }
    }
}

impl Default for DeviceInit {
    fn default() -> Self {
        Self {
            device: core::ptr::null_mut(),
            error: Error::none,
        }
    }
}

impl Default for SwapchainFrame {
    fn default() -> Self {
        Self {
            render_view: core::ptr::null_mut(),
            extent: uint32x2::default(),
        }
    }
}

impl Default for TextureDesc {
    fn default() -> Self {
        Self {
            type_: TextureType::two_d,
            extent: uint32x3 { x: 1, y: 1, z: 1 },
            mip_levels: 1,
            layer_count: 1,
            format: Format::rgba8_unorm,
            mutable_format: false,
            usage: TextureUsage::sampled,
        }
    }
}

impl Default for RenderViewDesc {
    fn default() -> Self {
        Self {
            mip_level: 0,
            slice: 0,
        }
    }
}

impl Default for TextureDescriptorDesc {
    fn default() -> Self {
        Self {
            format: Format::undefined,
            aspect: TextureAspect::automatic,
            base_mip: 0,
            mip_count: 0,
            base_layer: 0,
            layer_count: 0,
        }
    }
}

impl Default for TextureCopyDesc {
    fn default() -> Self {
        Self {
            mip_level: 0,
            base_slice: 0,
            slice_count: 0,
            offset: uint32x3::default(),
            extent: uint32x3::default(),
            row_pitch_bytes: 0,
            slice_pitch_bytes: 0,
        }
    }
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            min_filter: Filter::linear,
            mag_filter: Filter::linear,
            mip_filter: Filter::linear,
            address_u: AddressMode::repeat,
            address_v: AddressMode::repeat,
            address_w: AddressMode::repeat,
            anisotropic: false,
            compare_enabled: false,
            compare: CompareOp::less_equal,
        }
    }
}

impl Default for BlendComponentState {
    fn default() -> Self {
        Self {
            source: BlendFactor::one,
            destination: BlendFactor::zero,
            operation: BlendOp::add,
        }
    }
}

impl Default for BlendState {
    fn default() -> Self {
        Self {
            enabled: false,
            color: BlendComponentState::default(),
            alpha: BlendComponentState::default(),
        }
    }
}

impl Default for ColorTargetDesc {
    fn default() -> Self {
        Self {
            format: Format::undefined,
            blend: BlendState::default(),
            write_mask: 0xf,
        }
    }
}

impl Default for RasterizationState {
    fn default() -> Self {
        Self {
            cull: CullMode::none,
            depth_bias_constant: 0.0,
            depth_bias_clamp: 0.0,
            depth_bias_slope: 0.0,
        }
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            min_depth: 0.0,
            max_depth: 1.0,
        }
    }
}

impl Default for Scissor {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }
    }
}

impl Default for StencilFaceState {
    fn default() -> Self {
        Self {
            compare: CompareOp::always,
            fail: StencilOp::keep,
            pass: StencilOp::keep,
            depth_fail: StencilOp::keep,
            reference: 0,
        }
    }
}

impl Default for DepthStencilState {
    fn default() -> Self {
        Self {
            depth_test: false,
            depth_write: false,
            depth_compare: CompareOp::less_equal,
            stencil_test: false,
            stencil_read_mask: 0xff,
            stencil_write_mask: 0xff,
            front: StencilFaceState::default(),
            back: StencilFaceState::default(),
        }
    }
}

impl Default for ShaderStage {
    fn default() -> Self {
        Self {
            code: ByteSpan::default(),
            entry_point: c"main".as_ptr(),
            threadgroup_size: uint32x3 { x: 1, y: 1, z: 1 },
        }
    }
}

impl Default for GraphicsPSODesc {
    fn default() -> Self {
        Self {
            vertex: ShaderStage::default(),
            fragment: ShaderStage::default(),
            color_targets: Span::empty(),
            depth_format: Format::undefined,
            stencil_format: Format::undefined,
            rasterization: RasterizationState::default(),
        }
    }
}

impl Default for MeshPSODesc {
    fn default() -> Self {
        Self {
            task: ShaderStage::default(),
            mesh: ShaderStage::default(),
            fragment: ShaderStage::default(),
            color_targets: Span::empty(),
            depth_format: Format::undefined,
            stencil_format: Format::undefined,
            rasterization: RasterizationState::default(),
        }
    }
}

impl Default for ClearColor {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }
}

impl Default for ColorAttachment {
    fn default() -> Self {
        Self {
            render_view: core::ptr::null_mut(),
            load: LoadOp::load,
            store: StoreOp::store,
            clear: ClearColor::default(),
        }
    }
}

impl Default for DepthAttachment {
    fn default() -> Self {
        Self {
            render_view: core::ptr::null_mut(),
            load: LoadOp::load,
            store: StoreOp::store,
            clear: 1.0,
        }
    }
}

impl Default for StencilAttachment {
    fn default() -> Self {
        Self {
            render_view: core::ptr::null_mut(),
            load: LoadOp::load,
            store: StoreOp::store,
            clear: 0,
        }
    }
}

impl Default for RenderingDesc {
    fn default() -> Self {
        Self {
            colors: Span::empty(),
            depth: DepthAttachment::default(),
            stencil: StencilAttachment::default(),
        }
    }
}
