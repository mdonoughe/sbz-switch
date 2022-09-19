use futures::channel::mpsc;
use futures::Stream;
use windows::core::GUID;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback};

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::AudioEndpointVolumeCallback;
use crate::com::ComObject;

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
                    self.event_context.data1,
                    self.event_context.data2,
                    self.event_context.data3,
                    self.event_context.data4[0],
                    self.event_context.data4[1],
                    self.event_context.data4[2],
                    self.event_context.data4[3],
                    self.event_context.data4[4],
                    self.event_context.data4[5],
                    self.event_context.data4[6],
                    self.event_context.data4[7]
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
    pub fn new(volume: ComObject<IAudioEndpointVolume>) -> windows::core::Result<Self> {
        let (tx, rx) = mpsc::unbounded();

        unsafe {
            let callback: IAudioEndpointVolumeCallback =
                AudioEndpointVolumeCallback::new(tx).into();

            (*volume).RegisterControlChangeNotify(callback.clone())?;

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
            (*self.volume)
                .UnregisterControlChangeNotify(&*self.callback)
                .unwrap();
        }
    }
}
