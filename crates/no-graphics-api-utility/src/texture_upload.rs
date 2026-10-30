//! Rust port of upstream `NoGraphicsAPIUtility/texture_upload.hpp`.
//!
//! Splits a tightly packed source region into staging-sized, granularity-aligned texture copies.
//! Combined depth/stencil formats are unsupported, and pitches must be zero (descripe the region with
//! `offset`/`extent` instead).

use no_graphics_api_sys as sys;
use no_graphics_api_sys::{Texture, TextureCopyDesc, TextureDesc, TextureType};

use crate::upload_queue::UploadQueue;

/// Standalone counterpart of `UploadQueue::upload_texture` for whole-mip or sub-region uploads.
///
/// `description` must match `destination`; `source` covers one complete region with no pitch padding
/// and must fit the queue's staging capacity. Splits respect the queue's texture granularity.
///
/// Panics when the description, copy region, or source size is inconsistent.
///
/// # Safety
/// `destination` must be a live texture created by the queue's device and match `description`.
pub unsafe fn upload_texture(
    queue: &mut UploadQueue,
    destination: *mut Texture,
    description: &TextureDesc,
    source: &[u8],
    copy: &TextureCopyDesc,
) {
    assert!(
        !destination.is_null(),
        "texture upload requires a destination"
    );
    assert!(
        copy.mip_level < description.mip_levels,
        "mip level exceeds the texture description"
    );
    assert!(
        copy.row_pitch_bytes == 0 && copy.slice_pitch_bytes == 0,
        "the source must be tightly packed"
    );

    let capacity = queue.capacity();
    let mut format = sys::TextureFormatInfo::default();
    unsafe { sys::get_texture_format_info(description.format, &mut format) };
    assert!(format.bytes_per_block != 0, "unsupported texture format");
    assert!(
        capacity >= u64::from(format.bytes_per_block),
        "staging capacity is smaller than one texture block"
    );
    assert!(
        !(format.depth && format.stencil),
        "combined depth/stencil formats are unsupported"
    );

    let mut region = *copy;
    let mip_width = (description.extent.x >> copy.mip_level).max(1);
    let mip_height = (description.extent.y >> copy.mip_level).max(1);
    let mip_depth = (description.extent.z >> copy.mip_level).max(1);
    if region.extent.x == 0 {
        region.extent.x = mip_width - region.offset.x;
    }
    if region.extent.y == 0 {
        region.extent.y = mip_height - region.offset.y;
    }
    if region.extent.z == 0 {
        region.extent.z = mip_depth - region.offset.z;
    }
    if region.slice_count == 0 {
        region.slice_count = description.layer_count - region.base_slice;
    }

    let volume = description.type_ == TextureType::three_d;
    let slices = if volume {
        region.extent.z
    } else {
        region.slice_count
    };
    let columns = region.extent.x.div_ceil(format.block_extent.x);
    let rows = region.extent.y.div_ceil(format.block_extent.y);
    let row_bytes = u64::from(columns) * u64::from(format.bytes_per_block);
    let slice_bytes = row_bytes * u64::from(rows);
    assert!(
        region.extent.x != 0 && region.extent.y != 0 && slices != 0,
        "empty upload region"
    );
    assert_eq!(
        source.len() as u64,
        slice_bytes * u64::from(slices),
        "source size does not match the upload region"
    );

    let mut granularity = queue.texture_granularity();
    if !volume {
        granularity.z = 1;
    }
    let tile = texture_upload_tile(
        sys::uint32x3::new(columns, rows, slices),
        granularity,
        format.bytes_per_block,
        capacity,
    );
    debug_assert!(tile.x != 0 && tile.y != 0 && tile.z != 0);

    let block = format.block_extent;
    let mut slice = 0u32;
    while slice < slices {
        let mut part = region;
        part.base_slice = if volume { 0 } else { region.base_slice + slice };
        part.offset.z = if volume { region.offset.z + slice } else { 0 };
        let part_slices = tile.z.min(slices - slice);
        part.extent.z = if volume { part_slices } else { 1 };
        part.slice_count = if volume { 1 } else { part_slices };

        let mut row = 0u32;
        while row < rows {
            part.offset.y = region.offset.y + row * block.y;
            let part_rows = tile.y.min(rows - row);
            part.extent.y = part_rows * block.y;
            let remaining_y = region.extent.y - row * block.y;
            if part.extent.y > remaining_y {
                part.extent.y = remaining_y;
            }

            let mut column = 0u32;
            while column < columns {
                let part_columns = tile.x.min(columns - column);
                part.offset.x = region.offset.x + column * block.x;
                part.extent.x = part_columns * block.x;
                let remaining_x = region.extent.x - column * block.x;
                if part.extent.x > remaining_x {
                    part.extent.x = remaining_x;
                }

                let packed_row_bytes = u64::from(part_columns) * u64::from(format.bytes_per_block);
                let packed_slice_bytes = packed_row_bytes * u64::from(part_rows);
                let staging = queue.reserve(packed_slice_bytes * u64::from(part_slices));

                if part_columns == columns && part_rows == rows {
                    // The whole region is contiguous in the source, slices included.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            source.as_ptr(),
                            staging.cpu,
                            staging.size as usize,
                        )
                    };
                } else {
                    for z in 0..part_slices {
                        let input_offset = (u64::from(slice) + u64::from(z)) * slice_bytes
                            + u64::from(row) * row_bytes
                            + u64::from(column) * u64::from(format.bytes_per_block);
                        let input = unsafe { source.as_ptr().add(input_offset as usize) };
                        let output = unsafe {
                            staging
                                .cpu
                                .add((u64::from(z) * packed_slice_bytes) as usize)
                        };
                        if part_columns == columns {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    input,
                                    output,
                                    packed_slice_bytes as usize,
                                )
                            };
                        } else {
                            for y in 0..part_rows {
                                unsafe {
                                    core::ptr::copy_nonoverlapping(
                                        input.add((u64::from(y) * row_bytes) as usize),
                                        output.add((u64::from(y) * packed_row_bytes) as usize),
                                        packed_row_bytes as usize,
                                    )
                                };
                            }
                        }
                    }
                }

                let commands = queue.begin();
                unsafe {
                    sys::copy_memory_to_texture(
                        commands,
                        crate::upload_queue::gpu_range_of(&staging),
                        destination,
                        &part,
                    )
                };
                queue.bump_operations();

                column += tile.x;
            }
            row += tile.y;
        }
        slice += tile.z;
    }
}

fn texture_upload_tile(
    blocks: sys::uint32x3,
    granularity: sys::uint32x3,
    bytes_per_block: u32,
    capacity: u64,
) -> sys::uint32x3 {
    let granularity = sys::uint32x3::new(
        if granularity.x == 0 {
            blocks.x
        } else {
            granularity.x
        },
        if granularity.y == 0 {
            blocks.y
        } else {
            granularity.y
        },
        if granularity.z == 0 {
            blocks.z
        } else {
            granularity.z
        },
    );
    let minimum = sys::uint32x3::new(
        granularity.x.min(blocks.x),
        granularity.y.min(blocks.y),
        granularity.z.min(blocks.z),
    );
    assert!(
        capacity
            >= u64::from(minimum.x)
                * u64::from(minimum.y)
                * u64::from(minimum.z)
                * u64::from(bytes_per_block),
        "staging capacity cannot hold one granularity-sized block"
    );

    let mut tile = sys::uint32x3::default();
    let block = u64::from(bytes_per_block);

    let limit = capacity / (u64::from(minimum.y) * u64::from(minimum.z) * block);
    tile.x = if limit < u64::from(blocks.x) {
        limit as u32
    } else {
        blocks.x
    };
    if tile.x < blocks.x {
        tile.x -= tile.x % granularity.x;
    }

    let limit = capacity / (u64::from(tile.x) * u64::from(minimum.z) * block);
    tile.y = if limit < u64::from(blocks.y) {
        limit as u32
    } else {
        blocks.y
    };
    if tile.y < blocks.y {
        tile.y -= tile.y % granularity.y;
    }

    let limit = capacity / (u64::from(tile.x) * u64::from(tile.y) * block);
    tile.z = if limit < u64::from(blocks.z) {
        limit as u32
    } else {
        blocks.z
    };
    if tile.z < blocks.z {
        tile.z -= tile.z % granularity.z;
    }

    tile
}

#[cfg(test)]
mod tests {
    use super::*;
    use no_graphics_api_sys::uint32x3;

    #[test]
    fn tile_respects_capacity_and_granularity() {
        assert_eq!(
            texture_upload_tile(uint32x3::new(16, 8, 4), uint32x3::new(1, 1, 1), 1, 256),
            uint32x3::new(16, 8, 2)
        );
        assert_eq!(
            texture_upload_tile(uint32x3::new(64, 64, 1), uint32x3::new(4, 4, 1), 16, 4096),
            uint32x3::new(64, 4, 1)
        );
        assert_eq!(
            texture_upload_tile(uint32x3::new(8, 8, 2), uint32x3::new(0, 0, 0), 4, 512),
            uint32x3::new(8, 8, 2)
        );
    }
}
