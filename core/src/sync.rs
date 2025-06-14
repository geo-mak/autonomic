use std::pin::Pin;
use std::sync::atomic::Ordering::{self, Acquire, Release};
use std::sync::{Mutex, atomic::AtomicBool};
use std::task::{Context, Poll, Waker};

use futures::task::AtomicWaker;

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
    #[inline(always)]
    pub const fn notified(&self) -> Notification<'_> {
        Notification { notify: self }
    }
}

pub struct Notification<'a> {
    notify: &'a Signal,
}

impl Future for Notification<'_> {
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

pub struct Switch {
    on: AtomicBool,
    waker: AtomicWaker,
}

impl Switch {
    #[inline]
    pub const fn new(on: bool) -> Self {
        Self {
            on: AtomicBool::new(on),
            waker: AtomicWaker::new(),
        }
    }

    #[inline]
    pub fn set(&self, on: bool) {
        self.on.store(on, Ordering::Release);
        self.waker.wake();
    }

    #[inline]
    pub fn get(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub const fn on(&self) -> On<'_> {
        On { switch: self }
    }

    #[inline(always)]
    pub const fn off(&self) -> Off<'_> {
        Off { switch: self }
    }
}

pub struct On<'a> {
    switch: &'a Switch,
}

impl Future for On<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.switch.waker.register(cx.waker());
        if self.switch.on.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

pub struct Off<'a> {
    switch: &'a Switch,
}

impl Future for Off<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.switch.waker.register(cx.waker());
        if !self.switch.on.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

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

    #[tokio::test]
    async fn test_switch_on() {
        let switch = Arc::new(Switch::new(false));
        let s = switch.clone();

        let on_future = async move {
            s.on().await;
            assert!(s.get());
        };

        let task = tokio::spawn(on_future);

        tokio::task::yield_now().await;

        switch.set(true);

        // Task should complete once switch is set to true
        let result = timeout(Duration::from_millis(100), task).await;
        assert!(result.is_ok(), "on() future did not resolve in time");
    }

    #[tokio::test]
    async fn test_switch_off() {
        let switch = Arc::new(Switch::new(true));
        let s = switch.clone();

        let off_future = async move {
            s.off().await;
            assert!(!s.get());
        };

        let test_task = tokio::spawn(off_future);

        tokio::task::yield_now().await;

        switch.set(false);

        let result = timeout(Duration::from_millis(100), test_task).await;
        assert!(result.is_ok(), "off() future did not resolve in time");
    }
}
