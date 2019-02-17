use futures::{Async, Stream};
use futures::executor::{self, Notify, NotifyHandle, Spawn};

use std::ptr;
use std::mem;
use std::sync::{Arc, Mutex};

use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateEventW, SetEvent};
use winapi::um::winbase::INFINITE;

use crate::hresult::{check};

struct ComUnparkState {
    handles: Vec<usize>,
    refs: Vec<usize>,
}

impl Drop for ComUnparkState {
    fn drop(&mut self) {
        for handle in self.handles.iter() {
            unsafe { CloseHandle(*handle as *mut _) };
        }
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
        Self {
            state: Arc::new(Mutex::new(ComUnparkState {
                handles: Vec::new(),
                refs: Vec::new(),
            })),
        }
    }

    pub fn allocate_id(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        let handle = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null_mut()) as usize };
        let pos = state.handles.binary_search(&handle).err().unwrap();
        state.handles.insert(pos, handle);
        state.refs.insert(pos, 1);
        handle
    }

    pub fn park(&self) -> usize {
        let state = self.state.lock().unwrap();
        let which = unsafe {
            let mut which = mem::uninitialized();
            check(CoWaitForMultipleObjects(
                CWMO_DISPATCH_CALLS,
                INFINITE,
                state.handles.len() as u32,
                &state.handles[0] as *const usize as *const _,
                &mut which as *mut _,
            ))
            .expect("failed to wait for unpark");
            which
        };
        state.handles[which as usize]
    }
}

impl Notify for ComUnpark {
    fn notify(&self, id: usize) {
        unsafe {
            SetEvent(id as *mut _);
        }
    }

    fn clone_id(&self, id: usize) -> usize {
        let mut state = self.state.lock().unwrap();
        let pos = state
            .handles
            .binary_search(&id)
            .expect("tried to clone nonexistant id");
        state.refs[pos] += 1;
        id
    }

    fn drop_id(&self, id: usize) {
        let mut state = self.state.lock().unwrap();
        let pos = state
            .handles
            .binary_search(&id)
            .expect("tried to drop nonexistant id");
        let refs = &mut state.refs[pos];
        match *refs {
            1 => {
                state.refs.remove(pos);
                let handle = state.handles.remove(pos);
                unsafe { CloseHandle(handle as *mut _) };
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

impl<S, I, E> ComEventIterator<S> where S: Stream<Item = I, Error = E> {
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

impl<S, I, E> Iterator for ComEventIterator<S> where S: Stream<Item = I, Error = E> {
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
