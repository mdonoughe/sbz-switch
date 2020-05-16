use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::{executor, Stream};

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

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
    events: mpsc::UnboundedReceiver<VolumeNotification>,
    callback: ComObject<IAudioEndpointVolumeCallback>,
}

impl VolumeEvents {
    pub fn new(volume: ComObject<IAudioEndpointVolume>) -> Result<Self, Win32Error> {
        let (mut tx, rx) = mpsc::unbounded();

        unsafe {
            let callback = AudioEndpointVolumeCallback::wrap(move |e| {
                match executor::block_on(tx.send(VolumeNotification {
                    event_context: e.guidEventContext,
                    is_muted: e.bMuted != 0,
                    volume: e.fMasterVolume,
                })) {
                    Ok(()) => Ok(()),
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
                callback: ComObject::take(callback),
            })
        }
    }
}

impl Stream for VolumeEvents {
    type Item = VolumeNotification;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.events).poll_next(cx)
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
