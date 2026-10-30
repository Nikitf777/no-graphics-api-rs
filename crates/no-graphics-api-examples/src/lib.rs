//! Rust ports of the NoGraphicsAPI examples.
//!
//! Each example opens a window with `winit`, creates a swapchain-backed device with
//! `NoGraphicsAPI`, and renders until the window closes — the same flow as the upstream C++
//! examples. On Linux the local presentation patch restricts the window backend to Wayland.

pub mod cube;
pub mod deferred;
pub mod math;
pub mod support;
pub mod triangle;
