use futures::task::AtomicTask;
use futures::{Async, Poll, Stream};

use std::clone::Clone;
use std::mem;
use std::sync::{mpsc, Arc};

use slog::Logger;

use winapi::shared::winerror::E_ABORT;

use crate::com::event::ComEventIterator;
use crate::com::ComObject;
use crate::ctsndcr::{EventInfo, ICallback, IEventNotify, ISoundCore, Param};
use crate::hresult::{check, Win32Error};

use super::{SoundCoreFeature, SoundCoreParameter};

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
    inner: ComEventIterator<SoundCoreEvents>,
}

impl SoundCoreEventIterator {
    pub(crate) fn new(stream: SoundCoreEvents) -> Self {
        SoundCoreEventIterator {
            inner: ComEventIterator::new(stream)
        }
    }
}

impl Iterator for SoundCoreEventIterator {
    type Item = Result<SoundCoreEvent, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreEvent, Win32Error>> {
        self.inner.next()
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
