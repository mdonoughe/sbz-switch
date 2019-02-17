use futures::task::AtomicTask;
use futures::{Async, Poll, Stream};

use std::clone::Clone;
use std::fmt;
use std::sync::{mpsc, Arc};

use winapi::shared::guiddef::GUID;
use winapi::shared::winerror::E_ABORT;
use winapi::um::endpointvolume::{IAudioEndpointVolume, IAudioEndpointVolumeCallback};

use super::AudioEndpointVolumeCallback;
use crate::com::ComObject;
use crate::hresult::{check, Win32Error};

/// Describes a volume change event.
///
/// [Official documentation](https://docs.microsoft.com/en-us/windows/desktop/api/endpointvolume/ns-endpointvolume-audio_volume_notification_data)
pub struct VolumeNotification {
    /// The ID that was provided when changing the volume.
    pub event_context: GUID,
    /// Is the endpoint now muted?
    pub is_muted: bool,
    /// The new volume level of the endpoint.
    pub volume: f32,
}

impl fmt::Debug for VolumeNotification {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("VolumeNotification")
            .field(
                "event_context",
                &format_args!(
                    "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                    self.event_context.Data1,
                    self.event_context.Data2,
                    self.event_context.Data3,
                    self.event_context.Data4[0],
                    self.event_context.Data4[1],
                    self.event_context.Data4[2],
                    self.event_context.Data4[3],
                    self.event_context.Data4[4],
                    self.event_context.Data4[5],
                    self.event_context.Data4[6],
                    self.event_context.Data4[7]
                ),
            )
            .field("is_muted", &self.is_muted)
            .field("volume", &self.volume)
            .finish()
    }
}

pub(crate) struct VolumeEvents {
    volume: ComObject<IAudioEndpointVolume>,
    events: mpsc::Receiver<VolumeNotification>,
    task: Arc<AtomicTask>,
    callback: ComObject<IAudioEndpointVolumeCallback>,
}

impl VolumeEvents {
    pub fn new(volume: ComObject<IAudioEndpointVolume>) -> Result<Self, Win32Error> {
        let task = Arc::new(AtomicTask::new());
        let (tx, rx) = mpsc::channel();

        let tx_task = task.clone();
        unsafe {
            let callback = AudioEndpointVolumeCallback::wrap(move |e| {
                match tx.send(VolumeNotification {
                    event_context: e.guidEventContext,
                    is_muted: e.bMuted != 0,
                    volume: e.fMasterVolume,
                }) {
                    Ok(()) => {
                        tx_task.notify();
                        Ok(())
                    }
                    Err(_) => Err(Win32Error::new(E_ABORT)),
                }
            });

            let result = check((*volume).RegisterControlChangeNotify(callback));
            if let Err(error) = result {
                (*callback).Release();
                return Err(error);
            }

            Ok(Self {
                volume,
                events: rx,
                task,
                callback: ComObject::take(callback),
            })
        }
    }
}

impl Stream for VolumeEvents {
    type Item = VolumeNotification;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.task.register();
        match self.events.try_recv() {
            Ok(e) => Ok(Async::Ready(Some(e))),
            Err(mpsc::TryRecvError::Empty) => Ok(Async::NotReady),
            Err(mpsc::TryRecvError::Disconnected) => Ok(Async::Ready(None)),
        }
    }
}

impl Drop for VolumeEvents {
    fn drop(&mut self) {
        unsafe {
            check(
                (*self.volume).UnregisterControlChangeNotify(&*self.callback as *const _ as *mut _),
            )
            .unwrap();
        }
    }
}
