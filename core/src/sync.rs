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
    state: AtomicBool,
    waker: AtomicWaker,
}

impl Switch {
    #[inline]
    pub const fn new(on: bool) -> Self {
        Self {
            state: AtomicBool::new(on),
            waker: AtomicWaker::new(),
        }
    }

    #[inline]
    pub fn set(&self, new: bool) {
        self.state.store(new, Ordering::Release);
        self.waker.wake();
    }

    #[inline]
    pub fn get(&self) -> bool {
        self.state.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub const fn on(&self) -> On<'_> {
        On { switch: self }
    }
}

pub struct On<'a> {
    switch: &'a Switch,
}

impl Future for On<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.switch.waker.register(cx.waker());
        if self.switch.state.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
