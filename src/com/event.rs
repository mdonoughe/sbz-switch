use futures::task::{self, ArcWake};
use futures::Stream;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Com::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::System::WindowsProgramming::INFINITE;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

struct ComWaker {
    ready_event: HANDLE,
}

impl ArcWake for ComWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        unsafe {
            SetEvent(arc_self.ready_event);
        }
    }
}

impl Drop for ComWaker {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.ready_event) };
    }
}

unsafe impl Send for ComWaker {}
unsafe impl Sync for ComWaker {}

impl ComWaker {
    pub fn new() -> Self {
        let ready_event = unsafe { CreateEventW(None, false, false, None).unwrap() };
        Self { ready_event }
    }

    pub fn sleep(&self) {
        unsafe {
            CoWaitForMultipleObjects(CWMO_DISPATCH_CALLS.0 as u32, INFINITE, &[self.ready_event])
                .expect("failed to wait for wake");
        }
    }
}

pub struct ComEventIterator<S> {
    waker: Arc<ComWaker>,
    inner: S,
}

impl<S, I> ComEventIterator<S>
where
    S: Stream<Item = I>,
{
    pub fn new(stream: S) -> Self {
        ComEventIterator {
            waker: Arc::new(ComWaker::new()),
            inner: stream,
        }
    }
}

impl<S, I> Iterator for ComEventIterator<S>
where
    S: Stream<Item = I> + Unpin,
{
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        let waker = task::waker_ref(&self.waker);
        let context = &mut Context::from_waker(&*waker);
        loop {
            match Pin::new(&mut self.inner).poll_next(context) {
                Poll::Ready(Some(item)) => break Some(item),
                Poll::Ready(None) => break None,
                Poll::Pending => self.waker.sleep(),
            };
        }
    }
}
