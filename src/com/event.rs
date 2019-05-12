use futures::executor::{self, Notify, NotifyHandle, Spawn};
use futures::{Async, Stream};

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::{mem, ptr};

use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateEventW, ResetEvent, SetEvent};
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::HANDLE;

use crate::hresult::check;

struct ComUnparkState {
    handles: BTreeMap<usize, usize>,
    ready_event: HANDLE,
    ready: VecDeque<usize>,
    next_id: usize,
}

impl Drop for ComUnparkState {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.ready_event as *mut _) };
    }
}

#[derive(Clone)]
pub(crate) struct ComUnpark {
    state: Arc<Mutex<ComUnparkState>>,
}

unsafe impl Send for ComUnpark {}
unsafe impl Sync for ComUnpark {}

impl ComUnpark {
    pub fn new() -> Self {
        let ready_event = unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null_mut()) };
        Self {
            state: Arc::new(Mutex::new(ComUnparkState {
                handles: BTreeMap::new(),
                ready_event,
                ready: VecDeque::new(),
                next_id: 0,
            })),
        }
    }

    pub fn allocate_id(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        let id = state.next_id;
        state.next_id += 1;
        state.handles.insert(id, 1);
        id
    }

    pub fn park(&self) -> usize {
        let (ready_event, ready) = {
            let mut state = self.state.lock().unwrap();
            (state.ready_event, state.ready.pop_front())
        };
        if let Some(ready) = ready {
            return ready;
        }
        loop {
            unsafe {
                let mut which = mem::uninitialized();
                check(CoWaitForMultipleObjects(
                    CWMO_DISPATCH_CALLS,
                    INFINITE,
                    1,
                    &ready_event,
                    &mut which as *mut _,
                ))
                .expect("failed to wait for unpark");
            };
            let mut state = self.state.lock().unwrap();
            if let Some(ready) = state.ready.pop_front() {
                return ready;
            } else {
                unsafe {
                    ResetEvent(ready_event);
                }
            }
        }
    }
}

impl Notify for ComUnpark {
    fn notify(&self, id: usize) {
        let mut state = self.state.lock().unwrap();
        state.ready.push_back(id);
        unsafe {
            SetEvent(state.ready_event);
        }
    }

    fn clone_id(&self, id: usize) -> usize {
        let mut state = self.state.lock().unwrap();
        let refs = state
            .handles
            .get_mut(&id)
            .expect("tried to clone nonexistant id");
        *refs += 1;
        id
    }

    fn drop_id(&self, id: usize) {
        let mut state = self.state.lock().unwrap();
        let refs = state
            .handles
            .get_mut(&id)
            .expect("tried to drop nonexistant id");
        match *refs {
            1 => {
                state.handles.remove(&id);
            }
            other => {
                *refs = other - 1;
            }
        }
    }
}

impl Into<NotifyHandle> for ComUnpark {
    fn into(self) -> NotifyHandle {
        NotifyHandle::from(Arc::new(self))
    }
}

pub struct ComEventIterator<S> {
    park: ComUnpark,
    id: usize,
    inner: Spawn<S>,
}

impl<S, I, E> ComEventIterator<S>
where
    S: Stream<Item = I, Error = E>,
{
    pub fn new(stream: S) -> Self {
        let park = ComUnpark::new();
        let id = park.allocate_id();
        ComEventIterator {
            park,
            id,
            inner: executor::spawn(stream),
        }
    }
}

impl<S, I, E> Iterator for ComEventIterator<S>
where
    S: Stream<Item = I, Error = E>,
{
    type Item = Result<I, E>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.poll_stream_notify(&self.park, self.id) {
                Ok(Async::Ready(Some(item))) => break Some(Ok(item)),
                Ok(Async::Ready(None)) => break None,
                Ok(Async::NotReady) => {
                    self.park.park();
                }
                Err(error) => break Some(Err(error)),
            }
        }
    }
}

impl<S> Drop for ComEventIterator<S> {
    fn drop(&mut self) {
        self.park.drop_id(self.id);
    }
}
