//! Conversions between `glam` math and the upstream `shader_types` CPU/GPU layouts.
//!
//! `shader_types` matrices are row-major with 4-byte alignment (`float4x4 { rows: [float4; 4] }`),
//! while `glam` stores column-major matrices that may be 16-byte aligned. Root data always uses the
//! `shader_types` layout; conversion happens at the edge.

use glam::{Mat4, Vec3, Vec4};
use no_graphics_api_utility::shader_types::{float3, float4, float4x4};

/// Writes `matrix` into the row-major `float4x4` layout the shaders expect.
pub fn to_row_major(matrix: Mat4) -> float4x4 {
    let mut output = float4x4::default();
    for (row, slot) in output.rows.iter_mut().enumerate() {
        let value = matrix.row(row);
        *slot = float4::new(value.x, value.y, value.z, value.w);
    }
    output
}

pub fn to_float4(value: Vec4) -> float4 {
    float4::new(value.x, value.y, value.z, value.w)
}

pub fn to_float3(value: Vec3) -> float3 {
    float3::new(value.x, value.y, value.z)
}

/// Right-handed, Y-up view matrix, matching upstream `math::look_at_rh`.
pub fn look_at_rh(eye: glam::Vec3, center: glam::Vec3, up: glam::Vec3) -> Mat4 {
    glam::camera::rh::view::look_at_mat4(eye, center, up)
}

/// Right-handed perspective with Vulkan clip conventions (depth 0..1, NDC Y-down), matching upstream
/// `math::perspective_rh_zo` plus the Y-row flip the examples apply.
pub fn perspective_rh_zo(vertical_field_of_view: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    glam::camera::rh::proj::vulkan::perspective(vertical_field_of_view, aspect, near, far)
}
