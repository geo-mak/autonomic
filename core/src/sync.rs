use std::pin::Pin;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::{Mutex, atomic::AtomicBool};
use std::task::{Context, Poll, Waker};

// Notification signal for single waiter.
// Only the last waiter is notified.
pub struct Signal {
    notified: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Signal {
    #[inline]
    pub const fn new() -> Self {
        Self {
            notified: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    /// Notifies the waiter (if any).
    /// If no waiter is currently waiting, sets the flag so the next waiter wakes immediately.
    pub fn notify(&self) {
        self.notified.store(true, Release);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    /// Returns a Future that waits for the next notification.
    /// It supports repeated calls.
    // Only the last waiter is notified.
    #[inline(always)]
    pub const fn notified(&self) -> NotifyLast<'_> {
        NotifyLast { notify: self }
    }
}

pub struct NotifyLast<'a> {
    notify: &'a Signal,
}

impl Future for NotifyLast<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.notify.notified.swap(false, Acquire) {
            Poll::Ready(())
        } else {
            let mut waker_lock = self.notify.waker.lock().unwrap();
            *waker_lock = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signal_notifies_waiter() {
        use std::sync::Arc;

        let signal = Arc::new(Signal::new());
        let waiter = signal.clone();

        let wait_task = tokio::spawn(async move {
            waiter.notified().await;
        });

        tokio::task::yield_now().await;

        signal.notify();

        let result = tokio::time::timeout(std::time::Duration::from_millis(100), wait_task).await;
        assert!(result.is_ok(), "Signal did not notify in time");
    }
}
