//! CPU/GPU shared types from upstream `NoGraphicsAPIUtility/shader_types.h`.
//!
//! These mirror the Slang-side declarations byte for byte (row-major matrices, no padding) and are
//! meant to be used verbatim as root data or constant-buffer payloads. Math operators live in the
//! `math.hpp` header upstream and are intentionally not ported; use a Rust math library and convert
//! at the edge, keeping these types for anything that crosses into a shader.
//!
//! Layout is asserted in `tests/layout.rs` against the sizes and alignments of the upstream header.

#![allow(non_camel_case_types)]

macro_rules! vector_type {
    ($name:ident, $component:ty, ($($field:ident),+)) => {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name {
            $(pub $field: $component,)+
        }

        impl $name {
            pub const fn new($($field: $component),+) -> Self {
                Self { $($field),+ }
            }
        }
    };
}

/// Opaque binary16 storage: every bit pattern, including NaNs, crosses the CPU/GPU boundary unchanged.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct float16_t {
    pub bits: u16,
}

vector_type!(float2, f32, (x, y));
vector_type!(float3, f32, (x, y, z));
vector_type!(float4, f32, (x, y, z, w));

vector_type!(int2, i32, (x, y));
vector_type!(int3, i32, (x, y, z));
vector_type!(int4, i32, (x, y, z, w));

vector_type!(uint2, u32, (x, y));
vector_type!(uint3, u32, (x, y, z));
vector_type!(uint4, u32, (x, y, z, w));

macro_rules! matrix_type {
    ($name:ident, $row:ty, $rows:literal, $doc:literal) => {
        #[doc = $doc]
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $name {
            pub rows: [$row; $rows],
        }

        impl $name {
            pub const fn new(rows: [$row; $rows]) -> Self {
                Self { rows }
            }
        }

        impl core::ops::Index<usize> for $name {
            type Output = $row;

            fn index(&self, index: usize) -> &Self::Output {
                &self.rows[index]
            }
        }

        impl core::ops::IndexMut<usize> for $name {
            fn index_mut(&mut self, index: usize) -> &mut Self::Output {
                &mut self.rows[index]
            }
        }
    };
}

matrix_type!(float3x3, float3, 3, "3x3 matrix, row-major, `float3` rows.");
matrix_type!(float3x4, float4, 3, "3x4 matrix, row-major, `float4` rows.");
matrix_type!(float4x4, float4, 4, "4x4 matrix, row-major, `float4` rows.");

vector_type!(quaternion, f32, (x, y, z, w));

vector_type!(float16_t2, float16_t, (x, y));
vector_type!(float16_t3, float16_t, (x, y, z));
vector_type!(float16_t4, float16_t, (x, y, z, w));

vector_type!(int16_t2, i16, (x, y));
vector_type!(int16_t3, i16, (x, y, z));
vector_type!(int16_t4, i16, (x, y, z, w));

vector_type!(uint16_t2, u16, (x, y));
vector_type!(uint16_t3, u16, (x, y, z));
vector_type!(uint16_t4, u16, (x, y, z, w));
