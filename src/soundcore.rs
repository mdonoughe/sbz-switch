use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::str;
use std::sync::Arc;

use slog::Logger;
use winapi::shared::guiddef::GUID;
use winapi::shared::ntdef::HANDLE;
use winapi::shared::winerror::{E_ACCESSDENIED, E_FAIL};
use winapi::um::combaseapi::{CoCreateInstance, CLSCTX_ALL};
use winapi::um::combaseapi::{CoWaitForMultipleObjects, CWMO_DISPATCH_CALLS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::minwinbase::CRITICAL_SECTION;
use winapi::um::synchapi::CreateEventW;
use winapi::um::synchapi::{
    DeleteCriticalSection, EnterCriticalSection, InitializeCriticalSection, LeaveCriticalSection,
    SetEvent,
};
use winapi::um::winbase::INFINITE;
use winapi::Interface;

use ctsndcr::{
    EventInfo, FeatureInfo, HardwareInfo, ICallback, IEventNotify, ISoundCore, Param, ParamInfo,
    ParamValue,
};
use hresult::{check, Win32Error};

DEFINE_PROPERTYKEY!{PKEY_SOUNDCORECTL_CLSID,
0xc949c6aa, 0x132b, 0x4511,0xbb, 0x1b, 0x35, 0x26, 0x1a, 0x2a, 0x63, 0x33,
0}

#[derive(Debug)]
pub enum SoundCoreError {
    Win32(Win32Error),
    NotSupported,
}

impl fmt::Display for SoundCoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SoundCoreError::Win32(ref err) => write!(f, "Win32Error: {}", err),
            SoundCoreError::NotSupported => write!(f, "SoundCore not supported"),
        }
    }
}

impl Error for SoundCoreError {
    fn description(&self) -> &str {
        match *self {
            SoundCoreError::Win32(ref err) => err.description(),
            SoundCoreError::NotSupported => "SoundCore not supported",
        }
    }
    fn cause(&self) -> Option<&Error> {
        match *self {
            SoundCoreError::Win32(ref err) => Some(err),
            SoundCoreError::NotSupported => None,
        }
    }
}

impl From<Win32Error> for SoundCoreError {
    fn from(err: Win32Error) -> SoundCoreError {
        SoundCoreError::Win32(err)
    }
}

pub struct SoundCoreFeature {
    core: *mut ISoundCore,
    logger: Logger,
    context: u32,
    pub id: u32,
    pub description: String,
    pub version: String,
}

impl SoundCoreFeature {
    pub fn parameters(&self) -> SoundCoreParameterIterator {
        SoundCoreParameterIterator {
            target: self.core,
            logger: self.logger.clone(),
            context: self.context,
            feature: self,
            index: 0,
        }
    }
}

pub struct SoundCoreFeatureIterator {
    target: *mut ISoundCore,
    logger: Logger,
    context: u32,
    index: u32,
}

impl Iterator for SoundCoreFeatureIterator {
    type Item = Result<SoundCoreFeature, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreFeature, Win32Error>> {
        unsafe {
            let mut info: FeatureInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching feature .{}[{}]...",
                self.context,
                self.index
            );
            match check((*self.target).EnumFeatures(
                self.context,
                self.index,
                &mut info as *mut FeatureInfo,
            )) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code: code @ _, .. }) if code == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            trace!(
                self.logger,
                "Got feature .{}[{}] = {:?}",
                self.context,
                self.index,
                info
            );
            self.index += 1;
            match info.feature_id {
                0 => None,
                _ => {
                    let description_length = info
                        .description
                        .iter()
                        .position(|i| *i == 0)
                        .unwrap_or_else(|| info.description.len());
                    let version_length = info
                        .version
                        .iter()
                        .position(|i| *i == 0)
                        .unwrap_or_else(|| info.version.len());
                    Some(Ok(SoundCoreFeature {
                        core: self.target,
                        logger: self.logger.clone(),
                        context: self.context,
                        id: info.feature_id,
                        description: str::from_utf8(&info.description[0..description_length])
                            .unwrap()
                            .to_owned(),
                        version: str::from_utf8(&info.version[0..version_length])
                            .unwrap()
                            .to_owned(),
                    }))
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum SoundCoreParamValue {
    Float(f32),
    Bool(bool),
    U32(u32),
    I32(i32),
    None,
}

pub struct SoundCoreParameter<'a> {
    core: *mut ISoundCore,
    logger: Logger,
    context: u32,
    feature: &'a SoundCoreFeature,
    pub id: u32,
    pub kind: u32,
    pub size: Option<u32>,
    pub min_value: SoundCoreParamValue,
    pub max_value: SoundCoreParamValue,
    pub step_size: SoundCoreParamValue,
    pub attributes: u32,
    pub description: String,
}

impl<'a> SoundCoreParameter<'a> {
    pub fn get(&self) -> Result<SoundCoreParamValue, Win32Error> {
        // varsize -> not supported
        if self.kind == 5 {
            return Ok(SoundCoreParamValue::None);
        }
        unsafe {
            let param = Param {
                context: self.context,
                feature: self.feature.id,
                param: self.id,
            };
            let mut value: ParamValue = mem::uninitialized();
            trace!(
                self.logger,
                "Fetching parameter value .{}.{}.{}...",
                self.context,
                self.feature.id,
                self.id
            );
            match check((*self.core).GetParamValue(param, &mut value as *mut ParamValue)) {
                Ok(_) => {}
                Err(Win32Error { code: code @ _, .. }) if code == E_ACCESSDENIED => {
                    trace!(
                        self.logger,
                        "Got parameter value .{}.{}.{} = {}",
                        self.context,
                        self.feature.id,
                        self.id,
                        "ACCESSDENIED"
                    );
                    return Ok(SoundCoreParamValue::None);
                }
                Err(error) => return Err(error),
            };
            trace!(
                self.logger,
                "Got parameter value .{}.{}.{} = {:?}",
                self.context,
                self.feature.id,
                self.id,
                value
            );
            Ok(convert_param_value(&value))
        }
    }
    pub fn set(&self, value: &SoundCoreParamValue) -> Result<(), Win32Error> {
        unsafe {
            let param = Param {
                context: self.context,
                feature: self.feature.id,
                param: self.id,
            };
            let param_value = ParamValue {
                kind: match *value {
                    SoundCoreParamValue::Float(_) => 0,
                    SoundCoreParamValue::Bool(_) => 1,
                    SoundCoreParamValue::U32(_) => 2,
                    SoundCoreParamValue::I32(_) => 3,
                    _ => panic!("tried to set parameter with nothing"),
                },
                value: match *value {
                    SoundCoreParamValue::Float(f) => mem::transmute(f),
                    SoundCoreParamValue::Bool(b) => if b {
                        0xffff_ffff
                    } else {
                        0
                    },
                    SoundCoreParamValue::U32(u) => u,
                    SoundCoreParamValue::I32(i) => mem::transmute(i),
                    _ => panic!("tried to set parameter with nothing"),
                },
            };
            info!(
                self.logger,
                "Setting {}.{} = {:?}", self.feature.description, self.description, value
            );
            check((*self.core).SetParamValue(param, param_value))?;
            Ok(())
        }
    }
}

pub struct SoundCoreParameterIterator<'a> {
    target: *mut ISoundCore,
    logger: Logger,
    context: u32,
    feature: &'a SoundCoreFeature,
    index: u32,
}

fn convert_param_value(value: &ParamValue) -> SoundCoreParamValue {
    unsafe {
        match value.kind {
            0 => SoundCoreParamValue::Float(f32::from_bits(value.value)),
            1 => SoundCoreParamValue::Bool(value.value != 0),
            2 => SoundCoreParamValue::U32(value.value),
            3 => SoundCoreParamValue::I32(mem::transmute(value.value)),
            _ => SoundCoreParamValue::None,
        }
    }
}

impl<'a> Iterator for SoundCoreParameterIterator<'a> {
    type Item = Result<SoundCoreParameter<'a>, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreParameter<'a>, Win32Error>> {
        unsafe {
            let mut info: ParamInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching parameter .{}.{}[{}]...",
                self.context,
                self.feature.description,
                self.index
            );
            match check((*self.target).EnumParams(
                self.context,
                self.index,
                self.feature.id,
                &mut info as *mut ParamInfo,
            )) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code: code @ _, .. }) if code == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            trace!(
                self.logger,
                "Got parameter .{}.{}[{}] = {:?}",
                self.context,
                self.feature.description,
                self.index,
                info
            );
            self.index += 1;
            match info.param.feature {
                0 => None,
                _ => {
                    let description_length = info
                        .description
                        .iter()
                        .position(|i| *i == 0)
                        .unwrap_or_else(|| info.description.len());
                    Some(Ok(SoundCoreParameter {
                        core: self.target,
                        context: self.context,
                        feature: self.feature,
                        logger: self.logger.clone(),
                        id: info.param.param,
                        description: str::from_utf8(&info.description[0..description_length])
                            .unwrap()
                            .to_owned(),
                        attributes: info.param_attributes,
                        kind: info.param_type,
                        size: match info.param_type {
                            5 => Some(info.data_size),
                            _ => None,
                        },
                        min_value: convert_param_value(&info.min_value),
                        max_value: convert_param_value(&info.max_value),
                        step_size: convert_param_value(&info.step_size),
                    }))
                }
            }
        }
    }
}

pub struct SoundCore {
    sound_core: *mut ISoundCore,
    logger: Logger,
}

impl SoundCore {
    fn bind_hardware(&self, id: &str) -> Result<(), Win32Error> {
        trace!(self.logger, "Binding SoundCore to {}...", id);
        let mut buffer = [0; 260];
        for c in OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo {
            info_type: 0,
            info: buffer,
        };
        check(unsafe { (*self.sound_core).BindHardware(&info) })?;
        Ok(())
    }
    pub fn features(&self, context: u32) -> SoundCoreFeatureIterator {
        SoundCoreFeatureIterator {
            target: self.sound_core,
            logger: self.logger.clone(),
            context: context,
            index: 0,
        }
    }
    pub fn events(&self) -> Result<SoundCoreEventIterator, Win32Error> {
        unsafe {
            let mut event_notify: *mut IEventNotify = mem::uninitialized();
            check((*self.sound_core).QueryInterface(
                &IEventNotify::uuidof(),
                &mut event_notify as *mut *mut _ as *mut _,
            ))?;
            let (mut w32sink, iterator) = event_iterator(event_notify);
            let callback = ICallback::new(move |e| {
                // despite our ICallback belonging to STA COM,
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

impl Drop for SoundCore {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.logger, "Releasing SoundCore...");
            (*self.sound_core).Release();
        }
    }
}

fn create_sound_core(clsid: &GUID, logger: Logger) -> Result<SoundCore, SoundCoreError> {
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
            sound_core: sc,
            logger,
        })
    }
}

pub fn get_sound_core(clsid: &GUID, id: &str, logger: Logger) -> Result<SoundCore, SoundCoreError> {
    let core = create_sound_core(clsid, logger)?;
    core.bind_hardware(id)?;
    Ok(core)
}

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

pub struct SoundCoreEventIterator {
    event_notify: *mut IEventNotify,
    inner: Arc<UnsafeCell<SoundCoreEventIteratorState>>,
}

impl Iterator for SoundCoreEventIterator {
    type Item = Result<EventInfo, Win32Error>;

    fn next(&mut self) -> Option<Result<EventInfo, Win32Error>> {
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

            return result.map(Ok);
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

            (*self.event_notify).UnregisterEventCallback();
            (*self.event_notify).Release();
        }
    }
}

struct SoundCoreEventIteratorSink {
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

unsafe fn event_iterator(
    event_notify: *mut IEventNotify,
) -> (SoundCoreEventIteratorSink, SoundCoreEventIterator) {
    let inner = Arc::new(UnsafeCell::new(SoundCoreEventIteratorState::new()));
    (
        SoundCoreEventIteratorSink {
            inner: inner.clone(),
        },
        SoundCoreEventIterator {
            inner,
            event_notify,
        },
    )
}
