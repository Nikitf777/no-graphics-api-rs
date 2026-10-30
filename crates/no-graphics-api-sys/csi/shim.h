// C ABI shim over the C++ NoGraphicsAPI surface.
//
// Bindgen parses this header, so every declaration here defines the Rust FFI.
// Rules for keeping the ABI stable:
//   * Only C-compatible scalar parameters, pointers, and trivial aggregates (by pointer).
//   * Aggregates are never returned by value; the caller passes an out pointer.
//   * Spans are passed as (pointer, count) pairs instead of gpu::Span<T>.
//   * No default arguments: Rust supplies defaults through `impl Default`.
#pragma once

#include <NoGraphicsAPI/NoGraphicsAPI.hpp>

extern "C"
{
// Diagnostics.
void nga_set_error_callback(gpu::ErrorCallback callback);
void nga_get_texture_format_info(gpu::Format format, gpu::TextureFormatInfo* out);

// Device.
void nga_create_device(const gpu::DeviceDesc* desc, gpu::DeviceInit* out);
void nga_destroy_device(gpu::Device* device);
const gpu::DeviceCaps* nga_get_device_caps(const gpu::Device* device);
bool nga_supports_texture_format(const gpu::Device* device, gpu::Format format, gpu::TextureUsage usage);
void nga_get_drawable_extent(gpu::Device* device, gpu::uint32x2* out);
gpu::Error nga_set_swapchain_format(gpu::Device* device, gpu::Format format, gpu::ColorSpace color_space);

// Timeline semaphores.
gpu::TimelineSemaphore* nga_create_timeline_semaphore(gpu::Device* device, gpu::uint64 initial_value);
void nga_destroy_timeline_semaphore(gpu::TimelineSemaphore* semaphore);
gpu::uint64 nga_timeline_completed_value(const gpu::TimelineSemaphore* semaphore);
void nga_wait_timeline(gpu::TimelinePoint point);
void nga_wait_idle(gpu::Device* device);

// Presentation.
void nga_acquire(gpu::CommandBuffer* commands, gpu::SwapchainFrame* out);
void nga_submit_and_present(gpu::Device* device, const gpu::SubmitDesc* desc);

// GPU heaps.
void nga_create_gpu_heap(gpu::Device* device, gpu::uint64 byte_count, gpu::MemoryType memory, gpu::GpuHeap* out);
void nga_destroy_gpu_heap(const gpu::GpuHeap* heap);
void nga_gpu_range_of_heap(const gpu::GpuHeap* heap, gpu::GpuRange* out);
void nga_gpu_range_of_cpu_range(const gpu::GpuCpuRange<gpu::byte>* range, gpu::GpuRange* out);

// Texture heaps and textures.
void nga_create_texture_heap(gpu::Device* device, gpu::uint64 byte_count, gpu::TextureHeap* out);
void nga_destroy_texture_heap(const gpu::TextureHeap* heap);
void nga_get_texture_size_align(gpu::Device* device, const gpu::TextureDesc* desc, gpu::SizeAlign* out);
gpu::Texture* nga_create_texture(gpu::Device* device, const gpu::TextureDesc* desc, const gpu::TextureHeap* heap, gpu::uint64 offset);
void nga_destroy_texture(gpu::Texture* texture);
gpu::RenderView* nga_create_render_view(gpu::Texture* texture, const gpu::RenderViewDesc* desc);
void nga_destroy_render_view(gpu::RenderView* render_view);

// Texture descriptor heaps.
gpu::TextureDescriptorHeap* nga_create_texture_descriptor_heap(gpu::Device* device, gpu::uint32 capacity);
void nga_destroy_texture_descriptor_heap(gpu::TextureDescriptorHeap* heap);
void nga_write_texture_descriptor(gpu::TextureDescriptorHeap* heap, gpu::uint32 index, const gpu::Texture* texture, gpu::TextureDescriptorType type,
                                 const gpu::TextureDescriptorDesc* desc);
void nga_copy_texture_descriptors(const gpu::TextureDescriptorHeap* source, gpu::uint32 source_index, gpu::TextureDescriptorHeap* destination,
                                  gpu::uint32 destination_index, gpu::uint32 count);

// Sampler descriptor heaps.
gpu::SamplerDescriptorHeap* nga_create_sampler_descriptor_heap(gpu::Device* device, gpu::uint32 capacity);
void nga_destroy_sampler_descriptor_heap(gpu::SamplerDescriptorHeap* heap);
void nga_write_sampler_descriptor(gpu::SamplerDescriptorHeap* heap, gpu::uint32 index, const gpu::SamplerDesc* desc);
void nga_copy_sampler_descriptors(const gpu::SamplerDescriptorHeap* source, gpu::uint32 source_index, gpu::SamplerDescriptorHeap* destination,
                                  gpu::uint32 destination_index, gpu::uint32 count);

// Pipelines.
gpu::PSO* nga_create_graphics_pso(gpu::Device* device, const gpu::GraphicsPSODesc* desc);
gpu::PSO* nga_create_mesh_pso(gpu::Device* device, const gpu::MeshPSODesc* desc);
gpu::PSO* nga_create_compute_pso(gpu::Device* device, const gpu::ShaderStage* compute);
void nga_destroy_pso(gpu::PSO* pso);

// Command pools and submission.
gpu::CommandPool* nga_create_command_pool(gpu::Device* device, gpu::uint32 queue_index);
void nga_destroy_command_pool(gpu::CommandPool* pool);
void nga_reset_command_pool(gpu::CommandPool* pool);
gpu::CommandBuffer* nga_begin_commands(gpu::CommandPool* pool);
void nga_end_commands(gpu::CommandBuffer* commands);
void nga_submit(gpu::Device* device, const gpu::SubmitDesc* desc, gpu::uint32 queue_index);

// Command buffer state.
void nga_set_texture_descriptor_heap(gpu::CommandBuffer* commands, gpu::TextureDescriptorHeap* heap);
void nga_set_sampler_descriptor_heap(gpu::CommandBuffer* commands, gpu::SamplerDescriptorHeap* heap);
void nga_initialize_texture(gpu::CommandBuffer* commands, gpu::Texture* texture);

// Copies and barriers.
void nga_copy_memory(gpu::CommandBuffer* commands, gpu::GpuRange source, gpu::GpuRange destination);
void nga_copy_memory_to_texture(gpu::CommandBuffer* commands, gpu::GpuRange source, gpu::Texture* destination, const gpu::TextureCopyDesc* copy);
void nga_copy_texture_to_memory(gpu::CommandBuffer* commands, gpu::Texture* source, gpu::GpuRange destination, const gpu::TextureCopyDesc* copy);
void nga_barrier(gpu::CommandBuffer* commands, gpu::Stage before, gpu::Access before_access, gpu::Stage after, gpu::Access after_access);

// Timestamps.
void nga_write_timestamp(gpu::CommandBuffer* commands, gpu::uint64* cpu_destination, gpu::Stage stage);
void nga_read_timestamps(gpu::CommandPool* pool);

// Render passes and pipeline state.
void nga_begin_render_pass(gpu::CommandBuffer* commands, const gpu::RenderingDesc* desc, gpu::RenderingFlags flags);
void nga_end_render_pass(gpu::CommandBuffer* commands);
void nga_set_viewport(gpu::CommandBuffer* commands, const gpu::Viewport* viewport);
void nga_set_scissor(gpu::CommandBuffer* commands, const gpu::Scissor* scissor);
void nga_set_depth_stencil(gpu::CommandBuffer* commands, const gpu::DepthStencilState* state);
void nga_bind_pso(gpu::CommandBuffer* commands, const gpu::PSO* pso);

// Draws and dispatches.
void nga_draw(gpu::CommandBuffer* commands, const void* root, gpu::uint32 vertex_count, gpu::uint32 instance_count, gpu::uint32 first_vertex,
              gpu::uint32 first_instance);
void nga_draw_indexed(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange indices, gpu::IndexType type, gpu::uint32 index_count,
                      gpu::uint32 instance_count, gpu::uint32 first_index, gpu::int32 vertex_offset, gpu::uint32 first_instance);
void nga_draw_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments, gpu::uint32 draw_count, gpu::uint32 stride);
void nga_draw_indexed_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange indices, gpu::IndexType type, gpu::GpuRange arguments,
                               gpu::uint32 draw_count, gpu::uint32 stride);
void nga_dispatch(gpu::CommandBuffer* commands, const void* root, gpu::uint32x3 group_count);
void nga_dispatch_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments);
void nga_draw_meshlets(gpu::CommandBuffer* commands, const void* root, gpu::uint32x3 group_count);
void nga_draw_meshlets_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments, gpu::uint32 draw_count, gpu::uint32 stride);
}
