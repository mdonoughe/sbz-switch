use futures::executor::{self, Notify, NotifyHandle, Spawn};
use futures::task::AtomicTask;
use futures::{Async, Poll, Stream};

use std::clone::Clone;
use std::mem;
use std::ptr;
use std::sync::{mpsc, Arc, Mutex};

use slog::Logger;

use winapi::shared::winerror::E_ABORT;
use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateEventW, SetEvent};
use winapi::um::winbase::INFINITE;

use crate::com::ComObject;
use crate::ctsndcr::{EventInfo, ICallback, IEventNotify, ISoundCore, Param};
use crate::hresult::{check, Win32Error};

use super::{SoundCoreFeature, SoundCoreParameter};

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
struct ComUnpark {
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

pub(crate) struct SoundCoreEvents {
    event_notify: ComObject<IEventNotify>,
    events: mpsc::Receiver<EventInfo>,
    task: Arc<AtomicTask>,
    logger: Logger,
    core: ComObject<ISoundCore>,
}

impl SoundCoreEvents {
    pub fn new(
        event_notify: ComObject<IEventNotify>,
        core: ComObject<ISoundCore>,
        logger: Logger,
    ) -> Result<Self, Win32Error> {
        let task = Arc::new(AtomicTask::new());
        let (tx, rx) = mpsc::channel();

        let tx_task = task.clone();
        unsafe {
            let callback = ICallback::new(move |e| match tx.send(*e) {
                Ok(()) => {
                    tx_task.notify();
                    Ok(())
                }
                Err(_) => Err(Win32Error::new(E_ABORT)),
            });

            let result = check((*event_notify).RegisterEventCallback(0xff, callback));
            (*callback).Release();
            result?;
        }

        Ok(Self {
            event_notify,
            events: rx,
            task,
            core,
            logger,
        })
    }
}

impl Stream for SoundCoreEvents {
    type Item = SoundCoreEvent;
    type Error = Win32Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.task.register();
        match self.events.try_recv() {
            Ok(e) => match e.event {
                2 => unsafe {
                    let mut feature = mem::zeroed();
                    let feature = check(self.core.GetFeatureInfo(
                        0,
                        e.data_or_feature_id,
                        &mut feature,
                    ))
                    .map(|_| feature);
                    match feature {
                        Ok(feature) => {
                            let feature = SoundCoreFeature::new(
                                self.core.clone(),
                                self.logger.clone(),
                                0,
                                &feature,
                            );
                            let mut param = mem::zeroed();
                            let param = check(self.core.GetParamInfo(
                                Param {
                                    param: e.param_id,
                                    feature: e.data_or_feature_id,
                                    context: 0,
                                },
                                &mut param,
                            ))
                            .map(|_| param);
                            match param {
                                Ok(param) => {
                                    let param = SoundCoreParameter::new(
                                        self.core.clone(),
                                        feature.description.clone(),
                                        self.logger.clone(),
                                        &param,
                                    );
                                    Ok(Async::Ready(Some(SoundCoreEvent::ParamChange {
                                        feature,
                                        parameter: param,
                                    })))
                                }
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => Err(error),
                    }
                },
                _ => Ok(Async::Ready(Some(SoundCoreEvent::Unknown(e)))),
            },
            Err(mpsc::TryRecvError::Empty) => Ok(Async::NotReady),
            Err(mpsc::TryRecvError::Disconnected) => Ok(Async::Ready(None)),
        }
    }
}

impl Drop for SoundCoreEvents {
    fn drop(&mut self) {
        unsafe {
            self.event_notify.UnregisterEventCallback();
        }
    }
}

/// Iterates over events produced through the SoundCore API.
///
/// This allows a program to be notified of events such as switching
/// between headphones and speakers.
///
/// This iterator will block until the next event is available.
pub struct SoundCoreEventIterator {
    park: ComUnpark,
    id: usize,
    inner: Spawn<SoundCoreEvents>,
}

impl SoundCoreEventIterator {
    pub(crate) fn new(stream: SoundCoreEvents) -> Self {
        let park = ComUnpark::new();
        let id = park.allocate_id();
        SoundCoreEventIterator {
            park,
            id,
            inner: executor::spawn(stream),
        }
    }
}

impl Iterator for SoundCoreEventIterator {
    type Item = Result<SoundCoreEvent, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreEvent, Win32Error>> {
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

impl Drop for SoundCoreEventIterator {
    fn drop(&mut self) {
        self.park.drop_id(self.id);
    }
}

/// Describes an event produced through the SoundCore API.
#[derive(Debug)]
pub enum SoundCoreEvent {
    /// An event occurred that could not be translated.
    ///
    /// The original event information is included unmodified.
    Unknown(EventInfo),
    /// A parameter value has changed
    ParamChange {
        /// The feature that changed
        feature: SoundCoreFeature,
        /// The parameter that changed
        parameter: SoundCoreParameter,
    },
}
