#include "shim.h"

void nga_set_error_callback(gpu::ErrorCallback callback)
{
    gpu::set_error_callback(callback);
}

void nga_get_texture_format_info(gpu::Format format, gpu::TextureFormatInfo* out)
{
    *out = gpu::get_texture_format_info(format);
}

void nga_create_device(const gpu::DeviceDesc* desc, gpu::DeviceInit* out)
{
    *out = gpu::create_device(*desc);
}

void nga_destroy_device(gpu::Device* device)
{
    gpu::destroy_device(device);
}

const gpu::DeviceCaps* nga_get_device_caps(const gpu::Device* device)
{
    return &gpu::get_device_caps(device);
}

bool nga_supports_texture_format(const gpu::Device* device, gpu::Format format, gpu::TextureUsage usage)
{
    return gpu::supports_texture_format(device, format, usage);
}

void nga_get_drawable_extent(gpu::Device* device, gpu::uint32x2* out)
{
    *out = gpu::get_drawable_extent(device);
}

gpu::Error nga_set_swapchain_format(gpu::Device* device, gpu::Format format, gpu::ColorSpace color_space)
{
    return gpu::set_swapchain_format(device, format, color_space);
}

gpu::TimelineSemaphore* nga_create_timeline_semaphore(gpu::Device* device, gpu::uint64 initial_value)
{
    return gpu::create_timeline_semaphore(device, initial_value);
}

void nga_destroy_timeline_semaphore(gpu::TimelineSemaphore* semaphore)
{
    gpu::destroy_timeline_semaphore(semaphore);
}

gpu::uint64 nga_timeline_completed_value(const gpu::TimelineSemaphore* semaphore)
{
    return gpu::timeline_completed_value(semaphore);
}

void nga_wait_timeline(gpu::TimelinePoint point)
{
    gpu::wait_timeline(point);
}

void nga_wait_idle(gpu::Device* device)
{
    gpu::wait_idle(device);
}

void nga_acquire(gpu::CommandBuffer* commands, gpu::SwapchainFrame* out)
{
    *out = gpu::acquire(commands);
}

void nga_submit_and_present(gpu::Device* device, const gpu::SubmitDesc* desc)
{
    gpu::submit_and_present(device, *desc);
}

void nga_create_gpu_heap(gpu::Device* device, gpu::uint64 byte_count, gpu::MemoryType memory, gpu::GpuHeap* out)
{
    *out = gpu::create_gpu_heap(device, byte_count, memory);
}

void nga_destroy_gpu_heap(const gpu::GpuHeap* heap)
{
    gpu::destroy_gpu_heap(*heap);
}

void nga_gpu_range_of_heap(const gpu::GpuHeap* heap, gpu::GpuRange* out)
{
    *out = gpu::gpu_range(*heap);
}

void nga_gpu_range_of_cpu_range(const gpu::GpuCpuRange<gpu::byte>* range, gpu::GpuRange* out)
{
    *out = gpu::gpu_range(*range);
}

void nga_create_texture_heap(gpu::Device* device, gpu::uint64 byte_count, gpu::TextureHeap* out)
{
    *out = gpu::create_texture_heap(device, byte_count);
}

void nga_destroy_texture_heap(const gpu::TextureHeap* heap)
{
    gpu::destroy_texture_heap(*heap);
}

void nga_get_texture_size_align(gpu::Device* device, const gpu::TextureDesc* desc, gpu::SizeAlign* out)
{
    *out = gpu::get_texture_size_align(device, *desc);
}

gpu::Texture* nga_create_texture(gpu::Device* device, const gpu::TextureDesc* desc, const gpu::TextureHeap* heap, gpu::uint64 offset)
{
    return gpu::create_texture(device, *desc, *heap, offset);
}

void nga_destroy_texture(gpu::Texture* texture)
{
    gpu::destroy_texture(texture);
}

gpu::RenderView* nga_create_render_view(gpu::Texture* texture, const gpu::RenderViewDesc* desc)
{
    return gpu::create_render_view(texture, *desc);
}

void nga_destroy_render_view(gpu::RenderView* render_view)
{
    gpu::destroy_render_view(render_view);
}

gpu::TextureDescriptorHeap* nga_create_texture_descriptor_heap(gpu::Device* device, gpu::uint32 capacity)
{
    return gpu::create_texture_descriptor_heap(device, capacity);
}

void nga_destroy_texture_descriptor_heap(gpu::TextureDescriptorHeap* heap)
{
    gpu::destroy_texture_descriptor_heap(heap);
}

void nga_write_texture_descriptor(gpu::TextureDescriptorHeap* heap, gpu::uint32 index, const gpu::Texture* texture, gpu::TextureDescriptorType type,
                                  const gpu::TextureDescriptorDesc* desc)
{
    gpu::write_texture_descriptor(heap, index, texture, type, *desc);
}

void nga_copy_texture_descriptors(const gpu::TextureDescriptorHeap* source, gpu::uint32 source_index, gpu::TextureDescriptorHeap* destination,
                                  gpu::uint32 destination_index, gpu::uint32 count)
{
    gpu::copy_texture_descriptors(source, source_index, destination, destination_index, count);
}

gpu::SamplerDescriptorHeap* nga_create_sampler_descriptor_heap(gpu::Device* device, gpu::uint32 capacity)
{
    return gpu::create_sampler_descriptor_heap(device, capacity);
}

void nga_destroy_sampler_descriptor_heap(gpu::SamplerDescriptorHeap* heap)
{
    gpu::destroy_sampler_descriptor_heap(heap);
}

void nga_write_sampler_descriptor(gpu::SamplerDescriptorHeap* heap, gpu::uint32 index, const gpu::SamplerDesc* desc)
{
    gpu::write_sampler_descriptor(heap, index, *desc);
}

void nga_copy_sampler_descriptors(const gpu::SamplerDescriptorHeap* source, gpu::uint32 source_index, gpu::SamplerDescriptorHeap* destination,
                                  gpu::uint32 destination_index, gpu::uint32 count)
{
    gpu::copy_sampler_descriptors(source, source_index, destination, destination_index, count);
}

gpu::PSO* nga_create_graphics_pso(gpu::Device* device, const gpu::GraphicsPSODesc* desc)
{
    return gpu::create_graphics_pso(device, *desc);
}

gpu::PSO* nga_create_mesh_pso(gpu::Device* device, const gpu::MeshPSODesc* desc)
{
    return gpu::create_mesh_pso(device, *desc);
}

gpu::PSO* nga_create_compute_pso(gpu::Device* device, const gpu::ShaderStage* compute)
{
    return gpu::create_compute_pso(device, *compute);
}

void nga_destroy_pso(gpu::PSO* pso)
{
    gpu::destroy_pso(pso);
}

gpu::CommandPool* nga_create_command_pool(gpu::Device* device, gpu::uint32 queue_index)
{
    return gpu::create_command_pool(device, queue_index);
}

void nga_destroy_command_pool(gpu::CommandPool* pool)
{
    gpu::destroy_command_pool(pool);
}

void nga_reset_command_pool(gpu::CommandPool* pool)
{
    gpu::reset_command_pool(pool);
}

gpu::CommandBuffer* nga_begin_commands(gpu::CommandPool* pool)
{
    return gpu::begin_commands(pool);
}

void nga_end_commands(gpu::CommandBuffer* commands)
{
    gpu::end_commands(commands);
}

void nga_submit(gpu::Device* device, const gpu::SubmitDesc* desc, gpu::uint32 queue_index)
{
    gpu::submit(device, *desc, queue_index);
}

void nga_set_texture_descriptor_heap(gpu::CommandBuffer* commands, gpu::TextureDescriptorHeap* heap)
{
    gpu::set_texture_descriptor_heap(commands, heap);
}

void nga_set_sampler_descriptor_heap(gpu::CommandBuffer* commands, gpu::SamplerDescriptorHeap* heap)
{
    gpu::set_sampler_descriptor_heap(commands, heap);
}

void nga_initialize_texture(gpu::CommandBuffer* commands, gpu::Texture* texture)
{
    gpu::initialize_texture(commands, texture);
}

void nga_copy_memory(gpu::CommandBuffer* commands, gpu::GpuRange source, gpu::GpuRange destination)
{
    gpu::copy_memory(commands, source, destination);
}

void nga_copy_memory_to_texture(gpu::CommandBuffer* commands, gpu::GpuRange source, gpu::Texture* destination, const gpu::TextureCopyDesc* copy)
{
    gpu::copy_memory_to_texture(commands, source, destination, *copy);
}

void nga_copy_texture_to_memory(gpu::CommandBuffer* commands, gpu::Texture* source, gpu::GpuRange destination, const gpu::TextureCopyDesc* copy)
{
    gpu::copy_texture_to_memory(commands, source, destination, *copy);
}

void nga_barrier(gpu::CommandBuffer* commands, gpu::Stage before, gpu::Access before_access, gpu::Stage after, gpu::Access after_access)
{
    gpu::barrier(commands, before, before_access, after, after_access);
}

void nga_write_timestamp(gpu::CommandBuffer* commands, gpu::uint64* cpu_destination, gpu::Stage stage)
{
    gpu::write_timestamp(commands, cpu_destination, stage);
}

void nga_read_timestamps(gpu::CommandPool* pool)
{
    gpu::read_timestamps(pool);
}

void nga_begin_render_pass(gpu::CommandBuffer* commands, const gpu::RenderingDesc* desc, gpu::RenderingFlags flags)
{
    gpu::begin_render_pass(commands, *desc, flags);
}

void nga_end_render_pass(gpu::CommandBuffer* commands)
{
    gpu::end_render_pass(commands);
}

void nga_set_viewport(gpu::CommandBuffer* commands, const gpu::Viewport* viewport)
{
    gpu::set_viewport(commands, *viewport);
}

void nga_set_scissor(gpu::CommandBuffer* commands, const gpu::Scissor* scissor)
{
    gpu::set_scissor(commands, *scissor);
}

void nga_set_depth_stencil(gpu::CommandBuffer* commands, const gpu::DepthStencilState* state)
{
    gpu::set_depth_stencil(commands, *state);
}

void nga_bind_pso(gpu::CommandBuffer* commands, const gpu::PSO* pso)
{
    gpu::bind_pso(commands, pso);
}

void nga_draw(gpu::CommandBuffer* commands, const void* root, gpu::uint32 vertex_count, gpu::uint32 instance_count, gpu::uint32 first_vertex,
              gpu::uint32 first_instance)
{
    gpu::draw(commands, root, vertex_count, instance_count, first_vertex, first_instance);
}

void nga_draw_indexed(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange indices, gpu::IndexType type, gpu::uint32 index_count,
                      gpu::uint32 instance_count, gpu::uint32 first_index, gpu::int32 vertex_offset, gpu::uint32 first_instance)
{
    gpu::draw_indexed(commands, root, indices, type, index_count, instance_count, first_index, vertex_offset, first_instance);
}

void nga_draw_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments, gpu::uint32 draw_count, gpu::uint32 stride)
{
    gpu::draw_indirect(commands, root, arguments, draw_count, stride);
}

void nga_draw_indexed_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange indices, gpu::IndexType type, gpu::GpuRange arguments,
                               gpu::uint32 draw_count, gpu::uint32 stride)
{
    gpu::draw_indexed_indirect(commands, root, indices, type, arguments, draw_count, stride);
}

void nga_dispatch(gpu::CommandBuffer* commands, const void* root, gpu::uint32x3 group_count)
{
    gpu::dispatch(commands, root, group_count);
}

void nga_dispatch_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments)
{
    gpu::dispatch_indirect(commands, root, arguments);
}

void nga_draw_meshlets(gpu::CommandBuffer* commands, const void* root, gpu::uint32x3 group_count)
{
    gpu::draw_meshlets(commands, root, group_count);
}

void nga_draw_meshlets_indirect(gpu::CommandBuffer* commands, const void* root, gpu::GpuRange arguments, gpu::uint32 draw_count, gpu::uint32 stride)
{
    gpu::draw_meshlets_indirect(commands, root, arguments, draw_count, stride);
}
