use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use slog::Logger;

use winapi::shared::guiddef::GUID;
use winapi::um::combaseapi::{CoCreateInstance, CLSCTX_ALL};
use winapi::Interface;

use crate::com::{ComObject, ComScope};
use crate::ctsndcr::{HardwareInfo, ICallback, IEventNotify, ISoundCore};
use crate::hresult::{check, Win32Error};

use super::event::event_iterator;
use super::{SoundCoreError, SoundCoreEventIterator, SoundCoreFeatureIterator};

/// Provides control of Creative SoundBlaster features.
///
/// This is a wrapper around `ISoundCore`.
pub struct SoundCore {
    sound_core: ComObject<ISoundCore>,
    logger: Logger,
}

impl SoundCore {
    /// Creates a SoundCore wrapper for a device.
    ///
    /// `clsid` is the COM CLSID of the SoundCore implementation to use,
    /// and can be obtained from [`Endpoint.clsid()`](../media/Endpoint.t.html#method.clsid).
    ///
    /// `device_id` is the Windows device ID, and can be obtained from
    /// [`Endpoint.id()`](../media/Endpoint.t.html#method.id).
    pub fn for_device(
        clsid: &GUID,
        device_id: &str,
        logger: Logger,
    ) -> Result<SoundCore, SoundCoreError> {
        let _scope = ComScope::begin();
        let mut core = SoundCore::new(clsid, logger)?;
        core.bind_hardware(device_id)?;
        Ok(core)
    }
    #[allow(clippy::new_ret_no_self)]
    fn new(clsid: &GUID, logger: Logger) -> Result<SoundCore, SoundCoreError> {
        unsafe {
            let mut sc: *mut ISoundCore = mem::uninitialized();
            check(CoCreateInstance(
                clsid,
                ptr::null_mut(),
                CLSCTX_ALL,
                &ISoundCore::uuidof(),
                &mut sc as *mut *mut ISoundCore as *mut _,
            ))?;
            Ok(SoundCore {
                sound_core: ComObject::take(sc),
                logger,
            })
        }
    }
    fn bind_hardware(&mut self, id: &str) -> Result<(), Win32Error> {
        trace!(self.logger, "Binding SoundCore to {}...", id);
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
        SoundCoreFeatureIterator::new(self.sound_core.clone(), self.logger.clone(), context)
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
        unsafe {
            let mut event_notify: *mut IEventNotify = mem::uninitialized();
            check(self.sound_core.QueryInterface(
                &IEventNotify::uuidof(),
                &mut event_notify as *mut *mut _ as *mut _,
            ))?;
            let (mut w32sink, iterator) = event_iterator(
                ComObject::take(event_notify),
                self.sound_core.clone(),
                self.logger.clone(),
            );
            let callback = ICallback::new(move |e| {
                // despite our ICallback belonging to STA COM,
                // and events only firing while the main thread is processing events,
                // this executes on a different plain win32 thread,
                // so we need to marshal back to the correct thread
                // and we can't use std :(
                w32sink.send(*e);
                Ok(())
            });
            let result = check((*event_notify).RegisterEventCallback(0xff, callback));
            (*callback).Release();
            result?;
            Ok(iterator)
        }
    }
}
