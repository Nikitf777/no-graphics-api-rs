//! Rust port of upstream `NoGraphicsAPIUtility/delete_queue.hpp`.
//!
//! Defers resource destruction until a timeline value retires. Callbacks are plain Rust closures and
//! run on the thread that calls [`DeleteQueue::tick`] or [`DeleteQueue::drain`], so they can capture
//! owned resources without a trampoline or an inline storage limit.

use std::collections::VecDeque;

use no_graphics_api_sys::{TimelineSemaphore, timeline_completed_value};

struct Entry {
    retire_value: u64,
    callback: Box<dyn FnOnce()>,
}

struct TickingGuard<'a> {
    ticking: &'a mut bool,
}

impl Drop for TickingGuard<'_> {
    fn drop(&mut self) {
        *self.ticking = false;
    }
}

/// Runs deferred destructors once a timeline semaphore reaches their retire value.
///
/// Retire values must be enqueued in nondecreasing order. Every callback must run before the queue is
/// destroyed: call [`Self::drain`] after `wait_idle` and before `destroy_device`.
pub struct DeleteQueue {
    timeline: *mut TimelineSemaphore,
    capacity: u32,
    entries: VecDeque<Entry>,
    ticking: bool,
}

impl DeleteQueue {
    /// # Safety
    /// `timeline` must be a live semaphore that outlives the queue, and `capacity` must be nonzero.
    pub unsafe fn new(timeline: *mut TimelineSemaphore, capacity: u32) -> Self {
        assert!(
            !timeline.is_null(),
            "delete queue requires a timeline semaphore"
        );
        assert!(capacity != 0, "delete queue capacity must be nonzero");
        Self {
            timeline,
            capacity,
            entries: VecDeque::with_capacity(capacity as usize),
            ticking: false,
        }
    }

    /// Enqueues `callback` to run once the timeline reaches `retire_value`.
    ///
    /// Panics when the queue is full or when retire values are not nondecreasing.
    pub fn defer(&mut self, retire_value: u64, callback: impl FnOnce() + 'static) {
        assert!(
            (self.entries.len() as u32) < self.capacity,
            "delete queue capacity exhausted"
        );
        if let Some(last) = self.entries.back() {
            assert!(
                last.retire_value <= retire_value,
                "delete queue retire values must be nondecreasing"
            );
        }

        self.entries.push_back(Entry {
            retire_value,
            callback: Box::new(callback),
        });
    }

    /// Runs every callback whose retire value the timeline already reached.
    pub fn tick(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let completed_value = unsafe { timeline_completed_value(self.timeline) };
        self.collect(completed_value);
    }

    /// Runs every queued callback regardless of the timeline, in enqueue order.
    pub fn drain(&mut self) {
        self.collect(u64::MAX);
    }

    /// Number of callbacks still queued.
    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum number of queued callbacks.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    fn collect(&mut self, completed_value: u64) {
        assert!(
            !self.ticking,
            "delete queue callbacks cannot recursively collect the same queue"
        );

        self.ticking = true;
        let _guard = TickingGuard {
            ticking: &mut self.ticking,
        };
        while let Some(entry) = self.entries.front() {
            if entry.retire_value > completed_value {
                break;
            }

            let entry = self
                .entries
                .pop_front()
                .expect("delete queue entry vanished");
            (entry.callback)();
        }
    }
}

impl Drop for DeleteQueue {
    fn drop(&mut self) {
        // Mirrors upstream: pending callbacks are not run on destruction. Drain the queue first so the
        // resources they own are released while the device is still alive.
        debug_assert!(
            self.entries.is_empty(),
            "delete queue destroyed with pending callbacks"
        );
    }
}
