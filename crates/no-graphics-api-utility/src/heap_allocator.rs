//! Rust port of upstream `NoGraphicsAPIUtility/heap_allocator.hpp`.
//!
//! Splits one 16-byte-aligned storage range into variable-size allocations using the same two-level
//! bin index (3-bit mantissa) and neighbor coalescing as the C++ implementation. Offsets are stored
//! in 16-byte elements, so `capacity` is `storage.size / 16`.
//!
//! Deviation from upstream: exhausted allocation returns `None` instead of an empty aggregate with
//! an invalid token.

use core::mem::size_of;
use core::ptr::null_mut;

use no_graphics_api_sys::GpuCpuRange;

const UNUSED: u32 = u32::MAX;
const MANTISSA_BITS: u32 = 3;
const MANTISSA_VALUE: u32 = 1 << MANTISSA_BITS;
const MANTISSA_MASK: u32 = MANTISSA_VALUE - 1;
const BINS_PER_LEAF: u32 = 8;
const TOP_BIN_COUNT: usize = 32;
const LEAF_BIN_COUNT: usize = TOP_BIN_COUNT * BINS_PER_LEAF as usize;

/// One allocation returned by [`HeapAllocator::allocate`].
///
/// `token` identifies the allocation for [`HeapAllocator::free`] and must be freed by the allocator
/// that produced it.
pub struct HeapAllocation<T> {
    pub range: GpuCpuRange<T>,
    pub token: u32,
}

impl<T: Copy> Clone for HeapAllocation<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for HeapAllocation<T> {}

impl<T> core::fmt::Debug for HeapAllocation<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeapAllocation")
            .field("cpu", &self.range.cpu)
            .field("gpu", &self.range.gpu)
            .field("size", &self.range.size)
            .field("token", &self.token)
            .finish()
    }
}

#[derive(Clone, Copy)]
struct Node {
    offset: u32,
    size: u32,
    bin_previous: u32,
    bin_next: u32,
    neighbor_previous: u32,
    neighbor_next: u32,
    used: bool,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            bin_previous: UNUSED,
            bin_next: UNUSED,
            neighbor_previous: UNUSED,
            neighbor_next: UNUSED,
            used: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Range {
    pub(crate) offset: u32,
    pub(crate) token: u32,
}

fn size_to_bin_round_down(size: u32) -> u32 {
    if size < MANTISSA_VALUE {
        return size;
    }

    let mantissa_start_bit = 31 - size.leading_zeros() - MANTISSA_BITS;
    ((mantissa_start_bit + 1) << MANTISSA_BITS) | ((size >> mantissa_start_bit) & MANTISSA_MASK)
}

fn find_lowest_set_bit(bits: u32, first_bit: u32) -> u32 {
    if first_bit >= 32 {
        return UNUSED;
    }

    let bits = bits & (u32::MAX << first_bit);
    if bits == 0 {
        UNUSED
    } else {
        bits.trailing_zeros()
    }
}

fn offset_pointer<T>(pointer: *mut T, offset: u64) -> *mut T {
    if pointer.is_null() {
        return null_mut();
    }

    (pointer as usize).wrapping_add(offset as usize) as *mut T
}

/// Variable-size allocator over one GPU heap's CPU/GPU address pair.
pub struct HeapAllocator {
    storage: GpuCpuRange<u8>,
    ranges: RangeAllocator,
}

impl HeapAllocator {
    /// All allocations are 16-byte aligned and sized in 16-byte elements.
    pub const ALIGNMENT: u64 = 16;
    /// Largest supported storage size.
    pub const MAXIMUM_SIZE: u64 = u32::MAX as u64 * Self::ALIGNMENT;
    /// Largest supported concurrent allocation count.
    pub const MAXIMUM_ALLOCATION_COUNT: u32 = (u32::MAX - 1) / 2;

    /// # Safety
    /// `storage` must outlive the allocator, expose at least one non-null CPU or GPU address, align
    /// every exposed address to [`Self::ALIGNMENT`], and contain 16..[`Self::MAXIMUM_SIZE`] bytes.
    /// `max_allocations` must be in `1..=[`Self::MAXIMUM_ALLOCATION_COUNT`]`.
    pub unsafe fn new(storage: GpuCpuRange<u8>, max_allocations: u32) -> Self {
        let size = u64::from(storage.size);
        debug_assert!(!storage.cpu.is_null() || !storage.gpu.is_null());
        debug_assert!(size >= Self::ALIGNMENT);
        debug_assert!(size <= Self::MAXIMUM_SIZE);
        debug_assert!(
            storage.cpu.is_null() || (storage.cpu as usize) % Self::ALIGNMENT as usize == 0
        );
        debug_assert!(
            storage.gpu.is_null() || (storage.gpu as usize) % Self::ALIGNMENT as usize == 0
        );

        Self {
            storage,
            ranges: RangeAllocator::new(
                size.min(Self::MAXIMUM_SIZE),
                max_allocations,
                Self::ALIGNMENT,
            ),
        }
    }

    /// Reserves `byte_size` bytes. The returned range is byte-sized; the underlying reservation is
    /// rounded up to [`Self::ALIGNMENT`].
    pub fn allocate(&mut self, byte_size: u64) -> Option<HeapAllocation<u8>> {
        let range = self.ranges.allocate(byte_size)?;
        let byte_offset = u64::from(range.offset) * self.ranges.element_size();
        Some(HeapAllocation {
            range: GpuCpuRange::new(
                offset_pointer(self.storage.cpu, byte_offset),
                offset_pointer(self.storage.gpu, byte_offset),
                byte_size as usize,
            ),
            token: range.token,
        })
    }

    /// Typed variant of [`Self::allocate`]; the range size is `element_count * size_of::<T>()`.
    pub fn allocate_typed<T>(&mut self, element_count: u64) -> Option<HeapAllocation<T>> {
        assert!(core::mem::align_of::<T>() as u64 <= Self::ALIGNMENT);
        let byte_size = element_count * size_of::<T>() as u64;
        let allocation = self.allocate(byte_size)?;
        Some(HeapAllocation {
            range: GpuCpuRange::new(
                allocation.range.cpu.cast(),
                allocation.range.gpu.cast(),
                byte_size as usize,
            ),
            token: allocation.token,
        })
    }

    /// Releases one allocation returned by this allocator. Freeing the same token twice is a
    /// programming error.
    pub fn free<T>(&mut self, allocation: HeapAllocation<T>) {
        self.ranges.free(allocation.token);
    }

    /// Invalidates every allocation.
    pub fn reset(&mut self) {
        self.ranges.reset();
    }

    /// Number of live allocations.
    pub fn allocation_count(&self) -> u32 {
        self.ranges.allocation_count
    }
}

pub(crate) struct RangeAllocator {
    element_size: u64,
    capacity: u32,
    max_allocations: u32,
    allocation_count: u32,
    used_top_bins: u32,
    used_leaf_bins: [u8; TOP_BIN_COUNT],
    bin_indices: [u32; LEAF_BIN_COUNT],
    nodes: Vec<Node>,
    free_nodes: Vec<u32>,
}

impl RangeAllocator {
    pub(crate) fn new(byte_size: u64, max_allocations: u32, element_size: u64) -> Self {
        debug_assert!(
            element_size != 0 && element_size.is_power_of_two() && byte_size >= element_size
        );
        debug_assert!(
            max_allocations != 0 && max_allocations <= HeapAllocator::MAXIMUM_ALLOCATION_COUNT
        );
        let element_capacity = byte_size / element_size;
        debug_assert!(element_capacity <= u32::MAX as u64);

        let node_capacity = max_allocations * 2 + 1;
        let mut ranges = Self {
            element_size,
            capacity: element_capacity as u32,
            max_allocations,
            allocation_count: 0,
            used_top_bins: 0,
            used_leaf_bins: [0; TOP_BIN_COUNT],
            bin_indices: [UNUSED; LEAF_BIN_COUNT],
            nodes: vec![Node::default(); node_capacity as usize],
            free_nodes: Vec::with_capacity(node_capacity as usize),
        };
        ranges.reset();
        ranges
    }

    pub(crate) fn element_size(&self) -> u64 {
        self.element_size
    }

    pub(crate) fn allocate(&mut self, byte_size: u64) -> Option<Range> {
        debug_assert!(byte_size != 0);
        if self.allocation_count == self.max_allocations {
            return None;
        }

        let requested_elements = 1 + (byte_size - 1) / self.element_size;
        if requested_elements > u64::from(self.capacity) {
            return None;
        }
        let size = requested_elements as u32;
        let approximate_bin_index = size_to_bin_round_down(size);
        let mut node_index = self.bin_indices[approximate_bin_index as usize];
        while node_index != UNUSED && self.nodes[node_index as usize].size < size {
            node_index = self.nodes[node_index as usize].bin_next;
        }

        if node_index == UNUSED {
            let minimum_bin_index = approximate_bin_index + 1;
            let mut top_bin_index = minimum_bin_index / BINS_PER_LEAF;
            let mut leaf_bin_index = UNUSED;
            if top_bin_index < TOP_BIN_COUNT as u32 {
                leaf_bin_index = find_lowest_set_bit(
                    u32::from(self.used_leaf_bins[top_bin_index as usize]),
                    minimum_bin_index % BINS_PER_LEAF,
                );
            }

            if leaf_bin_index == UNUSED {
                top_bin_index = find_lowest_set_bit(self.used_top_bins, top_bin_index + 1);
                if top_bin_index == UNUSED {
                    return None;
                }
                leaf_bin_index =
                    u32::from(self.used_leaf_bins[top_bin_index as usize]).trailing_zeros();
            }
            node_index =
                self.bin_indices[(top_bin_index * BINS_PER_LEAF + leaf_bin_index) as usize];
        }

        let original_size = self.nodes[node_index as usize].size;
        let original_next_neighbor = self.nodes[node_index as usize].neighbor_next;
        debug_assert!(original_size >= size);

        self.remove_free_node(node_index);
        self.nodes[node_index as usize].size = size;
        self.nodes[node_index as usize].used = true;
        self.allocation_count += 1;

        if original_size != size {
            let remainder_index = self.acquire_node();
            let offset = self.nodes[node_index as usize].offset + size;
            let remainder = &mut self.nodes[remainder_index as usize];
            remainder.offset = offset;
            remainder.size = original_size - size;
            remainder.neighbor_previous = node_index;
            remainder.neighbor_next = original_next_neighbor;
            if original_next_neighbor != UNUSED {
                self.nodes[original_next_neighbor as usize].neighbor_previous = remainder_index;
            }
            self.nodes[node_index as usize].neighbor_next = remainder_index;
            self.insert_free_node(remainder_index);
        }

        Some(Range {
            offset: self.nodes[node_index as usize].offset,
            token: node_index,
        })
    }

    pub(crate) fn free(&mut self, token: u32) {
        debug_assert!(token < self.nodes.len() as u32);
        debug_assert!(self.nodes[token as usize].used);

        let previous_index = self.nodes[token as usize].neighbor_previous;
        if previous_index != UNUSED && !self.nodes[previous_index as usize].used {
            self.remove_free_node(previous_index);
            let previous_offset = self.nodes[previous_index as usize].offset;
            let previous_size = self.nodes[previous_index as usize].size;
            let previous_previous = self.nodes[previous_index as usize].neighbor_previous;
            let node = &mut self.nodes[token as usize];
            node.offset = previous_offset;
            node.size += previous_size;
            node.neighbor_previous = previous_previous;
            self.release_node(previous_index);
        }

        let next_index = self.nodes[token as usize].neighbor_next;
        if next_index != UNUSED && !self.nodes[next_index as usize].used {
            self.remove_free_node(next_index);
            let next_size = self.nodes[next_index as usize].size;
            let next_next = self.nodes[next_index as usize].neighbor_next;
            let node = &mut self.nodes[token as usize];
            node.size += next_size;
            node.neighbor_next = next_next;
            self.release_node(next_index);
        }

        let previous_index = self.nodes[token as usize].neighbor_previous;
        if previous_index != UNUSED {
            self.nodes[previous_index as usize].neighbor_next = token;
        }
        let next_index = self.nodes[token as usize].neighbor_next;
        if next_index != UNUSED {
            self.nodes[next_index as usize].neighbor_previous = token;
        }

        self.insert_free_node(token);
        self.allocation_count -= 1;
    }

    pub(crate) fn reset(&mut self) {
        self.allocation_count = 0;
        self.used_top_bins = 0;
        self.used_leaf_bins = [0; TOP_BIN_COUNT];
        self.bin_indices = [UNUSED; LEAF_BIN_COUNT];
        self.nodes.fill(Node::default());
        self.free_nodes.clear();
        self.free_nodes.extend(0..self.nodes.len() as u32);

        let node_index = self.acquire_node();
        self.nodes[node_index as usize].size = self.capacity;
        self.insert_free_node(node_index);
    }

    fn acquire_node(&mut self) -> u32 {
        debug_assert!(!self.free_nodes.is_empty());
        self.free_nodes
            .pop()
            .expect("range allocator node capacity exhausted")
    }

    fn release_node(&mut self, node_index: u32) {
        self.nodes[node_index as usize] = Node::default();
        self.free_nodes.push(node_index);
    }

    fn insert_free_node(&mut self, node_index: u32) {
        let size = self.nodes[node_index as usize].size;
        let bin_index = size_to_bin_round_down(size);
        let top_bin_index = (bin_index / BINS_PER_LEAF) as usize;
        let leaf_bin_index = bin_index % BINS_PER_LEAF;

        let bin_next = self.bin_indices[bin_index as usize];
        let node = &mut self.nodes[node_index as usize];
        node.used = false;
        node.bin_previous = UNUSED;
        node.bin_next = bin_next;
        if bin_next != UNUSED {
            self.nodes[bin_next as usize].bin_previous = node_index;
        }
        self.bin_indices[bin_index as usize] = node_index;
        self.used_leaf_bins[top_bin_index] |= 1u8 << leaf_bin_index;
        self.used_top_bins |= 1u32 << top_bin_index;
    }

    fn remove_free_node(&mut self, node_index: u32) {
        let node = self.nodes[node_index as usize];
        debug_assert!(!node.used);

        if node.bin_previous != UNUSED {
            self.nodes[node.bin_previous as usize].bin_next = node.bin_next;
            if node.bin_next != UNUSED {
                self.nodes[node.bin_next as usize].bin_previous = node.bin_previous;
            }
        } else {
            let bin_index = size_to_bin_round_down(node.size);
            let top_bin_index = (bin_index / BINS_PER_LEAF) as usize;
            let leaf_bin_index = bin_index % BINS_PER_LEAF;

            debug_assert_eq!(self.bin_indices[bin_index as usize], node_index);
            self.bin_indices[bin_index as usize] = node.bin_next;
            if node.bin_next != UNUSED {
                self.nodes[node.bin_next as usize].bin_previous = UNUSED;
            } else {
                self.used_leaf_bins[top_bin_index] &= !(1u8 << leaf_bin_index);
                if self.used_leaf_bins[top_bin_index] == 0 {
                    self.used_top_bins &= !(1u32 << top_bin_index);
                }
            }
        }

        let node = &mut self.nodes[node_index as usize];
        node.bin_previous = UNUSED;
        node.bin_next = UNUSED;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_index_matches_reference_layout() {
        assert_eq!(size_to_bin_round_down(0), 0);
        assert_eq!(size_to_bin_round_down(1), 1);
        assert_eq!(size_to_bin_round_down(7), 7);
        assert_eq!(size_to_bin_round_down(8), 8);
        assert_eq!(size_to_bin_round_down(9), 9);
        assert_eq!(size_to_bin_round_down(15), 15);
        assert_eq!(size_to_bin_round_down(16), 16);
        assert_eq!(size_to_bin_round_down(17), 16);
        assert_eq!(size_to_bin_round_down(31), 23);
        assert_eq!(size_to_bin_round_down(32), 24);
        assert_eq!(size_to_bin_round_down(63), 31);
        assert_eq!(size_to_bin_round_down(64), 32);
        assert_eq!(size_to_bin_round_down(255), 47);
        assert_eq!(size_to_bin_round_down(u32::MAX), 239);
    }

    #[test]
    fn lowest_set_bit_search() {
        assert_eq!(find_lowest_set_bit(0, 0), UNUSED);
        assert_eq!(find_lowest_set_bit(1, 0), 0);
        assert_eq!(find_lowest_set_bit(0b1010, 0), 1);
        assert_eq!(find_lowest_set_bit(0b1010, 2), 3);
        assert_eq!(find_lowest_set_bit(0b1010, 4), UNUSED);
        assert_eq!(find_lowest_set_bit(u32::MAX, 31), 31);
        assert_eq!(find_lowest_set_bit(u32::MAX, 32), UNUSED);
    }
}
