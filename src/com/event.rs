use futures::task::{self, ArcWake};
use futures::Stream;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{mem::MaybeUninit, ptr};

use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateEventW, SetEvent};
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::HANDLE;

use crate::hresult::check;

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
        unsafe { CloseHandle(self.ready_event as *mut _) };
    }
}

unsafe impl Send for ComWaker {}
unsafe impl Sync for ComWaker {}

impl ComWaker {
    pub fn new() -> Self {
        let ready_event = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null_mut()) };
        Self { ready_event }
    }

    pub fn sleep(&self) {
        unsafe {
            let mut which = MaybeUninit::uninit();
            check(CoWaitForMultipleObjects(
                CWMO_DISPATCH_CALLS,
                INFINITE,
                1,
                &self.ready_event,
                which.as_mut_ptr(),
            ))
            .expect("failed to wait for wake");
            which.assume_init();
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
