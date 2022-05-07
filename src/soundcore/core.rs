use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use tracing::instrument;
use winapi::shared::guiddef::GUID;
use winapi::um::combaseapi::{CoCreateInstance, CLSCTX_ALL};
use winapi::Interface;

use crate::com::{ComObject, ComScope};
use crate::ctsndcr::{HardwareInfo, IEventNotify, ISoundCore};
use crate::hresult::{check, Win32Error};

use super::event::{SoundCoreEventIterator, SoundCoreEvents};
use super::{SoundCoreError, SoundCoreFeatureIterator};

/// Provides control of Creative SoundBlaster features.
///
/// This is a wrapper around `ISoundCore`.
#[derive(Debug)]
pub struct SoundCore {
    sound_core: ComObject<ISoundCore>,
}

impl SoundCore {
    /// Creates a SoundCore wrapper for a device.
    ///
    /// `clsid` is the COM CLSID of the SoundCore implementation to use,
    /// and can be obtained from [`Endpoint.clsid()`](../media/Endpoint.t.html#method.clsid).
    ///
    /// `device_id` is the Windows device ID, and can be obtained from
    /// [`Endpoint.id()`](../media/Endpoint.t.html#method.id).
    pub fn for_device(clsid: &GUID, device_id: &str) -> Result<SoundCore, SoundCoreError> {
        let _scope = ComScope::begin();
        let mut core = SoundCore::new(clsid)?;
        core.bind_hardware(device_id)?;
        Ok(core)
    }
    #[allow(clippy::new_ret_no_self)]
    fn new(clsid: &GUID) -> Result<SoundCore, SoundCoreError> {
        unsafe {
            let mut sc = MaybeUninit::<*mut ISoundCore>::uninit();
            check(CoCreateInstance(
                clsid,
                ptr::null_mut(),
                CLSCTX_ALL,
                &ISoundCore::uuidof(),
                sc.as_mut_ptr() as *mut _,
            ))?;
            Ok(SoundCore {
                sound_core: ComObject::take(sc.assume_init()),
            })
        }
    }
    #[instrument(level = "trace")]
    fn bind_hardware(&mut self, id: &str) -> Result<(), Win32Error> {
        let mut buffer = [0; 260];
        for c in OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo {
            info_type: 0,
            info: buffer,
        };
        check(unsafe { self.sound_core.BindHardware(&info) })?;
        Ok(())
    }
    /// Returns an iterator over the features exposed by a device.
    pub fn features(&self, context: u32) -> SoundCoreFeatureIterator {
        SoundCoreFeatureIterator::new(self.sound_core.clone(), context)
    }
    /// Returns an iterator over events produced by the SoundCore API.
    ///
    /// This includes events such as the speaker configuration changing.
    ///
    /// Calling `next` will block the current thread until events are available.
    ///
    /// Events are only received during a call to `next`.
    /// Events are likely dropped if `next` is called infrequently.
    ///
    /// Event iterators with overlapping lifetimes may produce unexpected results
    /// because the SoundCore API for events does not seem to support registering
    /// multiple event handlers and then unregistering only one of them. Probably
    /// this is okay if done with multiple `SoundCore` instances.
    pub fn events(&self) -> Result<SoundCoreEventIterator, Win32Error> {
        Ok(SoundCoreEventIterator::new(self.event_stream()?))
    }

    pub(crate) fn event_stream(&self) -> Result<SoundCoreEvents, Win32Error> {
        unsafe {
            let mut event_notify = MaybeUninit::<*mut IEventNotify>::uninit();
            check(
                self.sound_core
                    .QueryInterface(&IEventNotify::uuidof(), event_notify.as_mut_ptr() as *mut _),
            )?;
            SoundCoreEvents::new(
                ComObject::take(event_notify.assume_init()),
                self.sound_core.clone(),
            )
        }
    }
}
