//! Rust port of upstream `NoGraphicsAPIUtility/upload_queue.hpp`.
//!
//! A fixed-capacity staging ring over one `GpuHeap`: uploads are memcpy'd into a mapped ring and
//! recorded as transfer commands, batches are submitted with timeline completion points, and staging
//! space is reclaimed when the GPU retires a batch.
//!
//! Externally synchronize the uploader and its queue. The queue index selects the staging/barrier
//! profile; transfer-only queues clamp texture copies to `DeviceCaps::copy_texture_granularity`.
//!
//! Deviation from upstream: construction returns `None` instead of producing an invalid object, and
//! the submission handoff is a boxed closure instead of a C function pointer plus context.

use core::ffi::c_void;
use core::ptr::null_mut;

use no_graphics_api_sys as sys;
use no_graphics_api_sys::{
    Access, CommandBuffer, CommandPool, Device, GpuCpuRange, GpuHeap, GpuRange, MemoryType, Span,
    Stage, SubmitDesc, Texture, TextureCopyDesc, TimelinePoint, barrier, begin_commands,
    copy_memory, copy_memory_to_texture, create_command_pool, create_gpu_heap,
    create_timeline_semaphore, destroy_command_pool, destroy_gpu_heap, destroy_timeline_semaphore,
    end_commands, get_device_caps, read_timestamps, reset_command_pool, submit,
    timeline_completed_value, wait_timeline, write_timestamp,
};

const UPLOAD_ACCESS: Access =
    Access::from_bits(Access::transfer_read.bits() | Access::transfer_write.bits());
const GPU_ACCESS: Access = Access::from_bits(
    Access::shader_read.bits()
        | Access::shader_write.bits()
        | Access::transfer_read.bits()
        | Access::transfer_write.bits()
        | Access::color_read.bits()
        | Access::color_write.bits()
        | Access::depth_stencil_read.bits()
        | Access::depth_stencil_write.bits()
        | Access::indirect_read.bits()
        | Access::index_read.bits()
        | Access::descriptor_read.bits(),
);
const COMPUTE_STAGES: Stage = Stage::from_bits(Stage::transfer.bits() | Stage::compute.bits());
const COMPUTE_ACCESS: Access = Access::from_bits(
    Access::shader_read.bits() | Access::shader_write.bits() | Access::descriptor_read.bits(),
);

/// Occupied bytes and pending work reported by [`UploadQueue::stats`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UploadQueueStats {
    pub capacity: u64,
    pub bytes_in_use: u64,
    pub peak_bytes: u64,
    pub submissions: u64,
    pub waits: u64,
    pub pending_batches: u32,
    pub pending_operations: u32,
}

/// Command buffer and staging range handed to an [`UploadQueue::upload_with_compute`] callback.
///
/// Record compute work between the caller's own bindings; the queue brackets it with barriers.
pub struct UploadFrame {
    pub commands: *mut CommandBuffer,
    pub staging: GpuCpuRange<u8>,
}

/// Optional submission handoff, replacing the default `submit` call.
pub type SubmitCallback = Box<dyn FnMut(*mut Device, *mut CommandBuffer, TimelinePoint, u32)>;

#[derive(Clone, Copy)]
struct Batch {
    pool: *mut CommandPool,
    end: u64,
    value: u64,
}

fn offset_pointer<T>(pointer: *mut T, offset: u64) -> *mut T {
    if pointer.is_null() {
        return null_mut();
    }

    (pointer as usize).wrapping_add(offset as usize) as *mut T
}

pub(crate) fn gpu_range_of(range: &GpuCpuRange<u8>) -> GpuRange {
    GpuRange {
        gpu: range.gpu.cast(),
        size: range.size,
    }
}

/// Deferred-staging upload ring for one queue.
pub struct UploadQueue {
    device: *mut Device,
    heap: GpuHeap,
    capacity: u64,
    completion: TimelinePoint,
    commands: *mut CommandBuffer,
    head: u64,
    tail: u64,
    batches: Vec<Batch>,
    queue_index: u32,
    upload_stages: Stage,
    upload_access: Access,
    queue_access: Access,
    texture_granularity: sys::uint32x3,
    retirement_first: u32,
    retirement_count: u32,
    operation_count: u32,
    submissions: u64,
    waits: u64,
    peak_bytes: u64,
    submit_callback: Option<SubmitCallback>,
}

impl UploadQueue {
    /// Staging alignment in bytes.
    pub const ALIGNMENT: u64 = 16;
    /// Recorded operations per batch before an automatic flush.
    pub const OPERATION_LIMIT: u32 = 4096;

    /// # Safety
    /// `device` must live until the queue is destroyed and the selected queue must be externally
    /// synchronized. `capacity` must be a nonzero multiple of [`Self::ALIGNMENT`], `queue_index` must
    /// be below `DeviceCaps::queue_count`, and `max_pending_batches` must be nonzero.
    ///
    /// Returns `None` when staging resources could not be created.
    pub unsafe fn new(
        device: *mut Device,
        capacity: u64,
        queue_index: u32,
        max_pending_batches: u32,
    ) -> Option<Self> {
        assert!(!device.is_null());
        assert!(
            capacity >= Self::ALIGNMENT && capacity % Self::ALIGNMENT == 0,
            "capacity must be a nonzero multiple of 16"
        );
        assert!(
            max_pending_batches != 0,
            "max_pending_batches must be nonzero"
        );

        let caps = unsafe { &*get_device_caps(device) };
        assert!(
            queue_index < caps.queue_count,
            "queue index exceeds the device queue count"
        );

        let mut upload_stages = Stage::transfer;
        let mut upload_access = UPLOAD_ACCESS;
        let mut queue_access = UPLOAD_ACCESS;
        let mut texture_granularity = sys::uint32x3::new(1, 1, 1);
        if queue_index < caps.general_queue_count + caps.compute_queue_count {
            upload_stages = COMPUTE_STAGES;
            upload_access = Access::from_bits(UPLOAD_ACCESS.bits() | COMPUTE_ACCESS.bits());
            queue_access = Access::from_bits(upload_access.bits() | Access::indirect_read.bits());
            if queue_index < caps.general_queue_count {
                queue_access = GPU_ACCESS;
            }
        } else {
            texture_granularity = caps.copy_texture_granularity;
        }

        let mut queue = Self {
            device,
            heap: GpuHeap::default(),
            capacity,
            completion: TimelinePoint::default(),
            commands: null_mut(),
            head: 0,
            tail: 0,
            batches: Vec::new(),
            queue_index,
            upload_stages,
            upload_access,
            queue_access,
            texture_granularity,
            retirement_first: 0,
            retirement_count: 0,
            operation_count: 0,
            submissions: 0,
            waits: 0,
            peak_bytes: 0,
            submit_callback: None,
        };

        unsafe { create_gpu_heap(device, capacity, MemoryType::cpu_visible, &mut queue.heap) };
        if queue.heap.owner.is_null() {
            return None;
        }

        queue.completion.semaphore = unsafe { create_timeline_semaphore(device, 0) };
        if queue.completion.semaphore.is_null() {
            queue.destroy();
            return None;
        }

        queue.batches.reserve(max_pending_batches as usize);
        for _ in 0..max_pending_batches {
            let pool = unsafe { create_command_pool(device, queue_index) };
            if pool.is_null() {
                queue.destroy();
                return None;
            }

            let commands = unsafe { begin_commands(pool) };
            if commands.is_null() {
                unsafe { destroy_command_pool(pool) };
                queue.destroy();
                return None;
            }
            unsafe { end_commands(commands) };
            unsafe { reset_command_pool(pool) };
            queue.batches.push(Batch {
                pool,
                end: 0,
                value: 0,
            });
        }

        Some(queue)
    }

    /// Flushes and waits for pending uploads, then releases every resource.
    ///
    /// Idempotent; also invoked by `Drop`. Finish external submissions that wait on
    /// [`Self::flush`] points before destroying the queue.
    pub fn destroy(&mut self) {
        self.release();
    }

    fn release(&mut self) {
        if !self.completion.semaphore.is_null() {
            self.wait();
        }

        for batch in self.batches.drain(..) {
            if !batch.pool.is_null() {
                unsafe { destroy_command_pool(batch.pool) };
            }
        }
        if !self.heap.owner.is_null() {
            unsafe { destroy_gpu_heap(&self.heap) };
        }
        if !self.completion.semaphore.is_null() {
            unsafe { destroy_timeline_semaphore(self.completion.semaphore) };
        }

        self.empty_in_place();
    }

    /// Resets the queue to the state of a destroyed instance without touching any handle.
    fn empty_in_place(&mut self) {
        self.device = null_mut();
        self.heap = GpuHeap::default();
        self.capacity = 0;
        self.completion = TimelinePoint::default();
        self.commands = null_mut();
        self.head = 0;
        self.tail = 0;
        self.batches.clear();
        self.queue_index = 0;
        self.upload_stages = Stage::transfer;
        self.upload_access = UPLOAD_ACCESS;
        self.queue_access = UPLOAD_ACCESS;
        self.texture_granularity = sys::uint32x3::new(1, 1, 1);
        self.retirement_first = 0;
        self.retirement_count = 0;
        self.operation_count = 0;
        self.submissions = 0;
        self.waits = 0;
        self.peak_bytes = 0;
        self.submit_callback = None;
    }

    /// Whether the queue still owns staging resources.
    pub fn is_valid(&self) -> bool {
        !self.device.is_null()
    }

    /// Copies `source` into staging and records buffer copies to `destination`.
    ///
    /// Sources larger than the ring are split. `source` is copied immediately.
    pub fn upload_buffer(&mut self, destination: GpuRange, source: &[u8]) {
        assert!(
            !self.completion.semaphore.is_null(),
            "upload queue is not initialized"
        );
        assert!(
            (source.len() as u64) <= u64::from(destination.size),
            "source exceeds the destination range"
        );

        let mut offset = 0u64;
        while offset < source.len() as u64 {
            let size = (source.len() as u64 - offset).min(self.capacity);
            let staging = self.reserve(size);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    source.as_ptr().add(offset as usize),
                    staging.cpu,
                    size as usize,
                )
            };

            let commands = self.begin();
            let destination = GpuRange {
                gpu: offset_pointer(destination.gpu.cast::<u8>(), offset).cast::<c_void>(),
                size: size as _,
            };
            unsafe { copy_memory(commands, gpu_range_of(&staging), destination) };
            self.operation_count += 1;
            offset += size;
        }
    }

    /// Copies one tightly packed or pitched region into a texture.
    ///
    /// # Safety
    /// `destination` must be a live texture created by this device.
    pub unsafe fn upload_texture(
        &mut self,
        destination: *mut Texture,
        source: &[u8],
        copy: &TextureCopyDesc,
    ) {
        assert!(!source.is_empty(), "texture upload requires a source");
        let staging = self.reserve(source.len() as u64);
        unsafe { core::ptr::copy_nonoverlapping(source.as_ptr(), staging.cpu, source.len()) };
        let commands = self.begin();
        unsafe { copy_memory_to_texture(commands, gpu_range_of(&staging), destination, copy) };
        self.operation_count += 1;
    }

    /// Records a timestamp marker into the current batch.
    ///
    /// # Safety
    /// `cpu_destination` must point to writable CPU memory that stays valid until the batch is
    /// reclaimed by `wait`/`reclaim`.
    pub unsafe fn write_timestamp(&mut self, cpu_destination: *mut u64) {
        let commands = self.begin();
        unsafe { write_timestamp(commands, cpu_destination, Stage::all_commands) };
    }

    /// Reserves staging, then runs `callback` with the recording command buffer and staging range.
    ///
    /// Requires a general or compute queue. The callback must not retain the command buffer or use
    /// the staging range after returning; the queue brackets it with transfer/compute barriers and
    /// counts one operation.
    pub fn upload_with_compute<F>(&mut self, byte_size: u64, callback: F)
    where
        F: FnOnce(UploadFrame),
    {
        assert!(
            self.upload_stages != Stage::transfer,
            "compute uploads require a general or compute queue"
        );

        let staging = self.reserve(byte_size);
        let commands = self.begin();
        unsafe {
            barrier(
                commands,
                COMPUTE_STAGES,
                Access::from_bits(Access::transfer_write.bits() | Access::shader_write.bits()),
                Stage::compute,
                COMPUTE_ACCESS,
            )
        };

        struct EndCompute<'a>(&'a mut UploadQueue);
        impl Drop for EndCompute<'_> {
            fn drop(&mut self) {
                self.0.end_compute();
            }
        }

        let guard = EndCompute(self);
        callback(UploadFrame { commands, staging });
        drop(guard);
    }

    fn end_compute(&mut self) {
        unsafe {
            barrier(
                self.commands,
                Stage::compute,
                Access::shader_write,
                COMPUTE_STAGES,
                Access::from_bits(
                    Access::transfer_read.bits()
                        | Access::transfer_write.bits()
                        | Access::shader_read.bits()
                        | Access::shader_write.bits()
                        | Access::descriptor_read.bits(),
                ),
            )
        };
        self.operation_count += 1;
    }

    /// Replaces the default submission with `callback`, or restores it with `None`.
    pub fn set_submit_callback(&mut self, callback: Option<SubmitCallback>) {
        self.submit_callback = callback;
    }

    /// Closes and submits the current batch, returning its completion point.
    ///
    /// Returns the latest completion point when nothing is pending.
    pub fn flush(&mut self) -> TimelinePoint {
        if self.commands.is_null() {
            return self.completion;
        }

        self.reclaim();
        unsafe {
            barrier(
                self.commands,
                self.upload_stages,
                self.upload_access,
                Stage::all_commands,
                self.queue_access,
            )
        };
        unsafe { end_commands(self.commands) };
        self.completion.value += 1;

        let commands = self.commands;
        let completion = self.completion;
        if let Some(callback) = self.submit_callback.as_mut() {
            callback(self.device, commands, completion, self.queue_index);
        } else {
            let mut commands = [commands];
            let desc = SubmitDesc {
                commands: Span::from_slice(&mut commands),
                waits: Span::empty(),
                completion,
            };
            unsafe { submit(self.device, &desc, self.queue_index) };
        }

        let index =
            ((self.retirement_first + self.retirement_count) % self.batches.len() as u32) as usize;
        self.batches[index].end = self.head;
        self.batches[index].value = self.completion.value;
        self.retirement_count += 1;
        self.submissions += 1;
        self.commands = null_mut();
        self.operation_count = 0;

        self.completion
    }

    /// Flushes pending uploads and waits until every submitted batch retires.
    pub fn wait(&mut self) {
        self.flush();
        self.reclaim();
        if self.retirement_count != 0 {
            unsafe { wait_timeline(self.completion) };
            self.waits += 1;
            self.reclaim();
        }
    }

    /// Reclaims completed batches without waiting or flushing; also retrieves their timestamps.
    pub fn reclaim(&mut self) {
        if self.retirement_count == 0 || self.completion.semaphore.is_null() {
            return;
        }

        let completed = unsafe { timeline_completed_value(self.completion.semaphore) };
        while self.retirement_count != 0
            && self.batches[self.retirement_first as usize].value <= completed
        {
            let batch = self.batches[self.retirement_first as usize];
            self.tail = batch.end;
            unsafe { read_timestamps(batch.pool) };
            unsafe { reset_command_pool(batch.pool) };
            self.retirement_first = (self.retirement_first + 1) % self.batches.len() as u32;
            self.retirement_count -= 1;
        }
    }

    pub fn stats(&self) -> UploadQueueStats {
        UploadQueueStats {
            capacity: self.capacity,
            bytes_in_use: self.head - self.tail,
            peak_bytes: self.peak_bytes,
            submissions: self.submissions,
            waits: self.waits,
            pending_batches: self.retirement_count,
            pending_operations: self.operation_count,
        }
    }

    pub(crate) fn capacity(&self) -> u64 {
        self.capacity
    }

    pub(crate) fn texture_granularity(&self) -> sys::uint32x3 {
        self.texture_granularity
    }

    pub(crate) fn bump_operations(&mut self) {
        self.operation_count += 1;
    }

    pub(crate) fn reserve(&mut self, byte_size: u64) -> GpuCpuRange<u8> {
        assert!(
            !self.completion.semaphore.is_null(),
            "upload queue is not initialized"
        );
        assert!(
            byte_size != 0 && byte_size <= self.capacity,
            "reservation exceeds the staging ring"
        );

        if self.operation_count == Self::OPERATION_LIMIT {
            self.flush();
        }

        loop {
            let mut offset =
                (self.head % self.capacity + Self::ALIGNMENT - 1) & !(Self::ALIGNMENT - 1);
            if byte_size > self.capacity - offset {
                offset = self.capacity;
            }
            let start = self.head - self.head % self.capacity + offset;
            if self.head == self.tail && self.retirement_count == 0 {
                self.head = start;
                self.tail = start;
            }

            if start + byte_size - self.tail <= self.capacity {
                self.head = start + byte_size;
                if self.head - self.tail > self.peak_bytes {
                    self.peak_bytes = self.head - self.tail;
                }
                let byte_offset = start % self.capacity;
                return GpuCpuRange::new(
                    offset_pointer(self.heap.range.cpu, byte_offset),
                    offset_pointer(self.heap.range.gpu, byte_offset),
                    byte_size as usize,
                );
            }

            let previous_count = self.retirement_count;
            self.reclaim();
            if self.retirement_count != previous_count {
                continue;
            }
            if !self.commands.is_null() {
                self.flush();
                continue;
            }
            self.wait_oldest();
        }
    }

    pub(crate) fn begin(&mut self) -> *mut CommandBuffer {
        assert!(
            !self.completion.semaphore.is_null(),
            "upload queue is not initialized"
        );

        if self.commands.is_null() {
            let batch_count = self.batches.len() as u32;
            if self.retirement_count == batch_count {
                self.reclaim();
                if self.retirement_count == batch_count {
                    self.wait_oldest();
                }
            }

            let index = ((self.retirement_first + self.retirement_count) % batch_count) as usize;
            let commands = unsafe { begin_commands(self.batches[index].pool) };
            assert!(!commands.is_null(), "command buffer allocation failed");
            self.commands = commands;
            unsafe {
                barrier(
                    commands,
                    Stage::all_commands,
                    self.queue_access,
                    self.upload_stages,
                    self.upload_access,
                )
            };
        }

        self.commands
    }

    fn wait_oldest(&mut self) {
        debug_assert!(self.retirement_count != 0);
        let point = TimelinePoint {
            semaphore: self.completion.semaphore,
            value: self.batches[self.retirement_first as usize].value,
        };
        unsafe { wait_timeline(point) };
        self.waits += 1;
        self.reclaim();
    }
}

impl Drop for UploadQueue {
    fn drop(&mut self) {
        self.release();
    }
}

impl core::fmt::Debug for UploadQueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UploadQueue")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}
