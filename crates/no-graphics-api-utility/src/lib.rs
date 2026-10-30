//! Safe Rust ports of the [NoGraphicsAPI](https://github.com/sebbbi/NoGraphicsAPI) utility helpers.
//!
//! The bindings live in `no-graphics-api-sys`. This crate reimplements the parts of
//! `NoGraphicsAPIUtility` that are pure host-side policy on top of them, so the only `unsafe` surface
//! stays in the bindings crate. Implemented here:
//!
//! | Module | Upstream |
//! | --- | --- |
//! | [`shader_types`] | `shader_types.h` |
//! | [`bump_allocator`] | `bump_allocator.hpp` |
//! | [`heap_allocator`] | `heap_allocator.hpp` |
//! | [`texture_allocator`] | `texture_allocator.hpp` |
//! | [`delete_queue`] | `delete_queue.hpp` |
//! | [`upload_queue`] | `upload_queue.hpp` |
//! | [`texture_upload`] | `texture_upload.hpp` |
//!
//! Not ported: `math.hpp` (operator library — use a Rust math crate and convert at the edge, keeping
//! the [`shader_types`] layouts for anything a shader reads) and `fixed_function.hpp` (its role is
//! replaced by boxed closures).
//!
//! These are ports, not bindings: behavior and API match upstream closely, but exhausted allocations
//! report `None` instead of an empty aggregate, and C++ move semantics become ordinary Rust moves.
//! As in upstream, allocators and queues are externally synchronized.

#![deny(unsafe_op_in_unsafe_fn)]
// The bindings expose `size_t`/`uint64` fields through platform `c_ulong`-style aliases. `u64::from`
// stays correct on every target bindgen supports, and is an identity conversion on 64-bit Unix.
#![allow(clippy::useless_conversion)]

/// The bindings this crate is implemented on, re-exported for convenience.
pub use no_graphics_api_sys;

pub mod bump_allocator;
pub mod delete_queue;
pub mod heap_allocator;
pub mod shader_types;
pub mod texture_allocator;
pub mod texture_upload;
pub mod upload_queue;

pub use bump_allocator::BumpAllocator;
pub use delete_queue::DeleteQueue;
pub use heap_allocator::{HeapAllocation, HeapAllocator};
pub use shader_types::*;
pub use texture_allocator::{PlacedTexture, TextureAllocator};
pub use upload_queue::{SubmitCallback, UploadFrame, UploadQueue, UploadQueueStats};
