//! Layout conformance for the CPU/GPU shared types.
//!
//! The expected values come from upstream `NoGraphicsAPIUtility/shader_types.h` (row-major matrices,
//! no explicit alignment). Re-check them whenever the vendored upstream commit changes.

use core::mem::{align_of, offset_of, size_of};

use no_graphics_api_utility::shader_types::*;

/// Byte offset of `row` inside `matrix`.
fn row_offset<M, R>(matrix: &M, row: &R) -> usize {
    row as *const R as usize - matrix as *const M as usize
}

#[test]
fn vector_layouts() {
    assert_eq!((size_of::<float2>(), align_of::<float2>()), (8, 4));
    assert_eq!((size_of::<float3>(), align_of::<float3>()), (12, 4));
    assert_eq!((size_of::<float4>(), align_of::<float4>()), (16, 4));

    assert_eq!((size_of::<int2>(), align_of::<int2>()), (8, 4));
    assert_eq!((size_of::<int3>(), align_of::<int3>()), (12, 4));
    assert_eq!((size_of::<int4>(), align_of::<int4>()), (16, 4));

    assert_eq!((size_of::<uint2>(), align_of::<uint2>()), (8, 4));
    assert_eq!((size_of::<uint3>(), align_of::<uint3>()), (12, 4));
    assert_eq!((size_of::<uint4>(), align_of::<uint4>()), (16, 4));

    assert_eq!((size_of::<quaternion>(), align_of::<quaternion>()), (16, 4));
}

#[test]
fn matrix_layouts() {
    assert_eq!((size_of::<float3x3>(), align_of::<float3x3>()), (36, 4));
    assert_eq!((size_of::<float3x4>(), align_of::<float3x4>()), (48, 4));
    assert_eq!((size_of::<float4x4>(), align_of::<float4x4>()), (64, 4));

    // Row stride equals the row size: matrices are tightly packed row-major arrays.
    let m3 = float3x3::default();
    let m34 = float3x4::default();
    let m44 = float4x4::default();
    assert_eq!(row_offset(&m3, &m3.rows[1]), 12);
    assert_eq!(row_offset(&m34, &m34.rows[1]), 16);
    assert_eq!(row_offset(&m44, &m44.rows[1]), 16);
    assert_eq!(row_offset(&m44, &m44.rows[3]), 48);
}

#[test]
fn half_and_short_layouts() {
    assert_eq!((size_of::<float16_t>(), align_of::<float16_t>()), (2, 2));
    assert_eq!((size_of::<float16_t2>(), align_of::<float16_t2>()), (4, 2));
    assert_eq!((size_of::<float16_t3>(), align_of::<float16_t3>()), (6, 2));
    assert_eq!((size_of::<float16_t4>(), align_of::<float16_t4>()), (8, 2));

    assert_eq!((size_of::<int16_t2>(), align_of::<int16_t2>()), (4, 2));
    assert_eq!((size_of::<int16_t3>(), align_of::<int16_t3>()), (6, 2));
    assert_eq!((size_of::<int16_t4>(), align_of::<int16_t4>()), (8, 2));

    assert_eq!((size_of::<uint16_t2>(), align_of::<uint16_t2>()), (4, 2));
    assert_eq!((size_of::<uint16_t3>(), align_of::<uint16_t3>()), (6, 2));
    assert_eq!((size_of::<uint16_t4>(), align_of::<uint16_t4>()), (8, 2));
}

#[test]
fn shader_struct_composition() {
    // A typical root payload: float4 + float4x4 + uint4 must stay tightly packed.
    #[repr(C)]
    struct Root {
        color: float4,
        transform: float4x4,
        ids: uint4,
    }

    assert_eq!(size_of::<Root>(), 16 + 64 + 16);
    assert_eq!(offset_of!(Root, transform), 16);
    assert_eq!(offset_of!(Root, ids), 80);
}
