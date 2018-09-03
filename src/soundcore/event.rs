use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::mem;
use std::ptr;
use std::sync::Arc;

use slog::Logger;

use winapi::shared::ntdef::HANDLE;
use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::minwinbase::CRITICAL_SECTION;
use winapi::um::synchapi::CreateEventW;
use winapi::um::synchapi::{
    DeleteCriticalSection, EnterCriticalSection, InitializeCriticalSection, LeaveCriticalSection,
    SetEvent,
};
use winapi::um::winbase::INFINITE;

use com::ComObject;
use ctsndcr::{EventInfo, IEventNotify, ISoundCore, Param};
use hresult::{check, Win32Error};

use super::{SoundCoreFeature, SoundCoreParameter};

struct SoundCoreEventIteratorState {
    next: VecDeque<EventInfo>,
    ready_read: HANDLE,
    lock: CRITICAL_SECTION,
    closed_read: bool,
    closed_write: bool,
}

impl SoundCoreEventIteratorState {
    fn new() -> Self {
        unsafe {
            let mut result = Self {
                next: VecDeque::new(),
                ready_read: CreateEventW(ptr::null_mut(), 0, 0, ptr::null_mut()),
                lock: mem::uninitialized(),
                closed_read: false,
                closed_write: false,
            };
            InitializeCriticalSection(&mut result.lock);
            return result;
        }
    }
}

impl Drop for SoundCoreEventIteratorState {
    fn drop(&mut self) {
        unsafe {
            DeleteCriticalSection(&mut self.lock);
            CloseHandle(self.ready_read);
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
    event_notify: ComObject<IEventNotify>,
    core: ComObject<ISoundCore>,
    inner: Arc<UnsafeCell<SoundCoreEventIteratorState>>,
    logger: Logger,
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

impl Iterator for SoundCoreEventIterator {
    type Item = Result<SoundCoreEvent, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreEvent, Win32Error>> {
        unsafe {
            let inner = &mut *self.inner.get();
            EnterCriticalSection(&mut inner.lock);

            loop {
                if !inner.next.is_empty() || inner.closed_write {
                    break;
                }
                LeaveCriticalSection(&mut inner.lock);

                let mut zero = mem::uninitialized();
                match check(CoWaitForMultipleObjects(
                    CWMO_DISPATCH_CALLS,
                    INFINITE,
                    1,
                    &inner.ready_read as *const _,
                    &mut zero as *mut _,
                )) {
                    Ok(_) => {}
                    Err(error) => return Some(Err(error)),
                }

                EnterCriticalSection(&mut inner.lock);
            }

            let result = inner.next.pop_front();

            LeaveCriticalSection(&mut inner.lock);

            match result {
                Some(result) => Some(match result.event {
                    2 => {
                        let mut feature = mem::zeroed();
                        let feature = check(self.core.GetFeatureInfo(
                            0,
                            result.data_or_feature_id,
                            &mut feature,
                        )).map(|_| feature);
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
                                        param: result.param_id,
                                        feature: result.data_or_feature_id,
                                        context: 0,
                                    },
                                    &mut param,
                                )).map(|_| param);
                                match param {
                                    Ok(param) => {
                                        let param = SoundCoreParameter::new(
                                            self.core.clone(),
                                            feature.description.clone(),
                                            self.logger.clone(),
                                            &param,
                                        );
                                        Ok(SoundCoreEvent::ParamChange {
                                            feature,
                                            parameter: param,
                                        })
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                            Err(error) => Err(error),
                        }
                    }
                    _ => Ok(SoundCoreEvent::Unknown(result)),
                }),
                None => None,
            }
        }
    }
}

impl Drop for SoundCoreEventIterator {
    fn drop(&mut self) {
        unsafe {
            let inner = &mut *self.inner.get();

            EnterCriticalSection(&mut inner.lock);

            inner.closed_read = true;
            inner.next.clear();

            LeaveCriticalSection(&mut inner.lock);

            self.event_notify.UnregisterEventCallback();
        }
    }
}

pub(crate) struct SoundCoreEventIteratorSink {
    inner: Arc<UnsafeCell<SoundCoreEventIteratorState>>,
}

impl SoundCoreEventIteratorSink {
    pub fn send(&mut self, item: EventInfo) {
        unsafe {
            let inner = &mut *self.inner.get();
            EnterCriticalSection(&mut inner.lock);

            if inner.closed_read {
                LeaveCriticalSection(&mut inner.lock);
                return;
            }

            inner.next.push_back(item);

            SetEvent(inner.ready_read);

            LeaveCriticalSection(&mut inner.lock);
        }
    }
}

unsafe impl Send for SoundCoreEventIteratorSink {}
unsafe impl Sync for SoundCoreEventIteratorSink {}

impl Drop for SoundCoreEventIteratorSink {
    fn drop(&mut self) {
        unsafe {
            let inner = &mut *self.inner.get();
            EnterCriticalSection(&mut inner.lock);

            inner.closed_write = true;

            SetEvent(inner.ready_read);

            LeaveCriticalSection(&mut inner.lock);
        }
    }
}

pub(crate) unsafe fn event_iterator(
    event_notify: ComObject<IEventNotify>,
    core: ComObject<ISoundCore>,
    logger: Logger,
) -> (SoundCoreEventIteratorSink, SoundCoreEventIterator) {
    let inner = Arc::new(UnsafeCell::new(SoundCoreEventIteratorState::new()));
    (
        SoundCoreEventIteratorSink {
            inner: inner.clone(),
        },
        SoundCoreEventIterator {
            inner,
            event_notify,
            core,
            logger,
        },
    )
}
