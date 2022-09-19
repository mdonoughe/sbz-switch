use futures::channel::mpsc;
use futures::Stream;

use std::clone::Clone;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::com::event::ComEventIterator;
use crate::com::ComObject;
use crate::ctsndcr::{Callback, EventInfo, IEventNotify, ISoundCore, Param};

use super::{SoundCoreFeature, SoundCoreParameter};

pub(crate) struct SoundCoreEvents {
    event_notify: ComObject<IEventNotify>,
    events: mpsc::UnboundedReceiver<EventInfo>,
    core: ComObject<ISoundCore>,
}

impl SoundCoreEvents {
    pub fn new(
        event_notify: ComObject<IEventNotify>,
        core: ComObject<ISoundCore>,
    ) -> windows::core::Result<Self> {
        let (tx, rx) = mpsc::unbounded();

        unsafe {
            let callback = Callback::new(tx);

            (*event_notify)
                .RegisterEventCallback(0xff, callback.into())
                .ok()?;
        }

        Ok(Self {
            event_notify,
            events: rx,
            core,
        })
    }
}

impl Stream for SoundCoreEvents {
    type Item = windows::core::Result<SoundCoreEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.events).poll_next(cx) {
            Poll::Ready(Some(e)) => Poll::Ready(Some(match e.event {
                2 => unsafe {
                    let mut feature = MaybeUninit::uninit();
                    match self
                        .core
                        .GetFeatureInfo(0, e.data_or_feature_id, feature.as_mut_ptr())
                        .ok()
                    {
                        Ok(()) => {
                            let feature =
                                SoundCoreFeature::new(self.core.clone(), 0, &feature.assume_init());
                            let mut param = MaybeUninit::uninit();
                            match self
                                .core
                                .GetParamInfo(
                                    Param {
                                        param: e.param_id,
                                        feature: e.data_or_feature_id,
                                        context: 0,
                                    },
                                    param.as_mut_ptr(),
                                )
                                .ok()
                            {
                                Ok(()) => {
                                    let param = SoundCoreParameter::new(
                                        self.core.clone(),
                                        feature.description.clone(),
                                        &param.assume_init(),
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
                },
                _ => Ok(SoundCoreEvent::Unknown(e)),
            })),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for SoundCoreEvents {
    fn drop(&mut self) {
        unsafe {
            self.event_notify.UnregisterEventCallback().ok().unwrap();
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
            inner: ComEventIterator::new(stream),
        }
    }
}

impl Iterator for SoundCoreEventIterator {
    type Item = windows::core::Result<SoundCoreEvent>;

    fn next(&mut self) -> Option<windows::core::Result<SoundCoreEvent>> {
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
