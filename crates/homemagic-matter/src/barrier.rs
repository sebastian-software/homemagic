use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// One controllable simulator dispatch barrier.
#[derive(Clone, Default)]
pub struct SimulatorBarrier {
    state: Arc<BarrierState>,
}

#[derive(Default)]
struct BarrierState {
    paused: AtomicBool,
    reached: AtomicBool,
    changed: Notify,
}

impl SimulatorBarrier {
    /// Pauses the next crossing and resets reached state.
    pub fn pause(&self) {
        self.state.reached.store(false, Ordering::SeqCst);
        self.state.paused.store(true, Ordering::SeqCst);
    }

    /// Returns whether an invocation is waiting at this barrier.
    #[must_use]
    pub fn is_reached(&self) -> bool {
        self.state.reached.load(Ordering::SeqCst)
    }

    /// Waits until an invocation reaches the paused barrier.
    pub async fn wait_until_reached(&self) {
        while !self.is_reached() {
            self.state.changed.notified().await;
        }
    }

    /// Releases a paused invocation.
    pub fn release(&self) {
        self.state.paused.store(false, Ordering::SeqCst);
        self.state.changed.notify_waiters();
    }

    pub(crate) async fn cross(&self) {
        if !self.state.paused.load(Ordering::SeqCst) {
            return;
        }
        self.state.reached.store(true, Ordering::SeqCst);
        self.state.changed.notify_waiters();
        while self.state.paused.load(Ordering::SeqCst) {
            self.state.changed.notified().await;
        }
    }
}

/// Dispatch barriers immediately before invocation and after acknowledgement.
#[derive(Clone, Default)]
pub struct SimulatorDispatchBarriers {
    /// Pauses before any simulated state-changing invocation.
    pub before_invoke: SimulatorBarrier,
    /// Pauses after simulated acknowledgement but before report delivery.
    pub after_acknowledgement: SimulatorBarrier,
}
