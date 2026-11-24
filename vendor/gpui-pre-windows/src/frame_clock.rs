//! Bookkeeping of the frame clock, the `VSyncProvider` thread started by
//! `WindowsPlatform::begin_vsync_thread`: when it waits for a vblank and which
//! windows it invalidates then.
//!
//! The clock runs only while a shown window wants a frame. GPUI asks for one
//! through the window's frame waker and `schedule_frame` whenever it has
//! something to draw or present: a dirty view, a next-frame callback such as
//! an animation step, a scene not yet presented. The clock invalidates that
//! window at the next vblank. Without demand it waits on [`FrameWake`], and an
//! idle window receives no `WM_PAINT`.
//!
//! A window that is painting also keeps the clock running, and a vblank that
//! passes during its paint invalidates it again, as the clock did when it
//! invalidated every shown window on every vblank: a frame that overruns its
//! vblank is followed by the next one as soon as it ends, not a vblank later.
//! After a vblank that invalidated a window the clock waits for the next one
//! as well, so that during an animation it wakes once per vblank, as it did
//! then, rather than also sleeping between each invalidation and its paint.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::{Condvar, Mutex};

#[cfg(test)]
mod tests;

/// Wakes an idle frame clock: a frame request, a paint, a window opened or
/// shown, shutdown. An epoch preserves a wake delivered between checking the
/// windows and entering the wait.
#[derive(Default)]
pub(crate) struct FrameWake {
    epoch: Mutex<u64>,
    changed: Condvar,
}

impl FrameWake {
    pub(crate) fn epoch(&self) -> u64 {
        *self.epoch.lock()
    }

    pub(crate) fn wake(&self) {
        let mut epoch = self.epoch.lock();
        *epoch = epoch.wrapping_add(1);
        self.changed.notify_all();
    }

    /// Waits until a wake after `epoch` or `stop`; false when `timeout` ran out.
    pub(crate) fn wait_since(&self, epoch: u64, stop: &AtomicBool, timeout: Duration) -> bool {
        let mut current = self.epoch.lock();
        !self
            .changed
            .wait_while_for(
                &mut current,
                |current| *current == epoch && !stop.load(Ordering::Acquire),
                timeout,
            )
            .timed_out()
    }
}

/// What one window needs from the frame clock, shared by the window (UI
/// thread) and the clock thread.
pub(crate) struct FrameDemand {
    /// GPUI asked for a frame; the next vblank invalidates the window.
    requested: AtomicBool,
    /// The window is inside `draw_window`.
    painting: AtomicBool,
    clock: Arc<FrameWake>,
}

impl FrameDemand {
    pub(crate) fn new(clock: Arc<FrameWake>) -> Self {
        Self {
            requested: AtomicBool::new(false),
            painting: AtomicBool::new(false),
            clock,
        }
    }

    /// Asks for an invalidation at the next vblank. Only a new request wakes
    /// the clock, so GPUI may repeat it freely.
    pub(crate) fn request(&self) {
        if !self.requested.swap(true, Ordering::AcqRel) {
            self.clock.wake();
        }
    }

    /// Marks the window as painting until the guard drops. The clock wakes to
    /// watch the vblanks during the paint: the window may be invalidated by a
    /// vblank before the paint started and leave the clock idle meanwhile.
    pub(crate) fn paint(&self) -> Painting<'_> {
        self.painting.store(true, Ordering::Release);
        self.clock.wake();
        Painting(self)
    }

    /// Clock thread: whether this window needs the next vblank.
    pub(crate) fn is_pending(&self) -> bool {
        self.requested.load(Ordering::Acquire) || self.painting.load(Ordering::Acquire)
    }

    /// Clock thread, at a vblank: whether to invalidate the window now. Takes
    /// the request; a window still painting is invalidated whether or not it
    /// asked, and its next `WM_PAINT` follows its paint at once.
    pub(crate) fn take_at_vblank(&self) -> bool {
        let requested = self.requested.swap(false, Ordering::AcqRel);
        requested || self.painting.load(Ordering::Acquire)
    }
}

pub(crate) struct Painting<'a>(&'a FrameDemand);

impl Drop for Painting<'_> {
    fn drop(&mut self) {
        self.0.painting.store(false, Ordering::Release);
    }
}
