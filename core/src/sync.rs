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

    /// Notifies the waiter (if any).
    /// This method is no-op if no waiter is currently waiting.
    /// No persistent notification for future waiters.
    pub fn notify_waiter(&self) {
        if let Some(waker) = self.waker.lock().unwrap().take() {
            self.notified.store(true, Release);
            waker.wake();
        }
    }

    /// Clears any persistent notification token from a previous activation.
    /// This ensures that future waiters will not be woken immediately.
    #[inline(always)]
    pub fn clear(&self) {
        self.notified.store(false, Release);
    }

    /// Returns a Future that waits for the next notification.
    /// It supports repeated calls.
    /// Only the last waiter is notified.
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
    async fn test_signal_notify() {
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

        // This should set the flag for future waiters.
        signal.notify();

        // A subsequent waiter should be notified immediately.
        let wait_task2 = tokio::spawn(async move {
            signal.notified().await;
        });

        let result2 = tokio::time::timeout(std::time::Duration::from_millis(50), wait_task2).await;
        assert!(
            result2.is_ok(),
            "Signal should have notified future waiter immediately"
        );
    }

    #[tokio::test]
    async fn test_signal_notify_waiter() {
        use std::sync::Arc;

        let signal = Arc::new(Signal::new());
        let waiter = signal.clone();

        let wait_task = tokio::spawn(async move {
            waiter.notified().await;
        });

        tokio::task::yield_now().await;

        signal.notify_waiter();

        let result = tokio::time::timeout(std::time::Duration::from_millis(100), wait_task).await;
        assert!(result.is_ok(), "Signal did not notify waiter in time");

        // This should be a no-op.
        signal.notify_waiter();

        // A subsequent waiter should not be notified immediately.
        let wait_task2 = tokio::spawn(async move {
            signal.notified().await;
        });

        let result2 = tokio::time::timeout(std::time::Duration::from_millis(50), wait_task2).await;
        // Future must be forcibly canceled.
        assert!(
            result2.is_err(),
            "Signal should not have notified when there was no waiter"
        );
    }
}
