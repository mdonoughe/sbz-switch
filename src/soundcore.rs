use std::error::Error;
use std::fmt;
use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::str;

use ole32::CoCreateInstance;
use slog::Logger;
use winapi::{CLSCTX_ALL, GUID};

use ctsndcr::{FeatureInfo, HardwareInfo, IID_SOUND_CORE, ISoundCore, Param, ParamInfo, ParamValue};
use hresult::{Win32Error, check};

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

pub struct SoundCoreFeature<'a> {
    core: *mut ISoundCore,
    logger: &'a Logger,
    context: u32,
    pub id: u32,
    pub description: String,
    pub version: String,
}

impl<'a> SoundCoreFeature<'a> {
    pub fn parameters(&'a self) -> SoundCoreParameterIterator<'a> {
        SoundCoreParameterIterator {
            target: self.core,
            logger: self.logger,
            context: self.context,
            feature: self,
            index: 0,
        }
    }
}

pub struct SoundCoreFeatureIterator<'a> {
    target: *mut ISoundCore,
    logger: &'a Logger,
    context: u32,
    index: u32,
}

impl<'a> Iterator for SoundCoreFeatureIterator<'a> {
    type Item = SoundCoreFeature<'a>;

    fn next(&mut self) -> Option<SoundCoreFeature<'a>> {
        unsafe {
            let mut info: FeatureInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching feature .{}[{}]...",
                self.context,
                self.index
            );
            (*self.target).EnumFeatures(self.context, self.index, &mut info as *mut FeatureInfo);
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
                    let description_length = info.description
                        .iter()
                        .position(|i| *i == 0)
                        .unwrap_or_else(|| info.description.len());
                    let version_length = info.version.iter().position(|i| *i == 0).unwrap_or_else(
                        || {
                            info.version.len()
                        },
                    );
                    Some(SoundCoreFeature {
                        core: self.target,
                        logger: self.logger,
                        context: self.context,
                        id: info.feature_id,
                        description: str::from_utf8(&info.description[0..description_length])
                            .unwrap()
                            .to_owned(),
                        version: str::from_utf8(&info.version[0..version_length])
                            .unwrap()
                            .to_owned(),
                    })
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
    logger: &'a Logger,
    context: u32,
    feature: &'a SoundCoreFeature<'a>,
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
    pub fn get(&self) -> SoundCoreParamValue {
        // varsize -> not supported
        if self.kind == 5 {
            return SoundCoreParamValue::None;
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
            (*self.core).GetParamValue(param, &mut value as *mut ParamValue);
            trace!(
                self.logger,
                "Got parameter value .{}.{}.{} = {:?}",
                self.context,
                self.feature.id,
                self.id,
                value
            );
            convert_param_value(&value)
        }
    }
    pub fn set(&self, value: &SoundCoreParamValue) {
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
                    SoundCoreParamValue::Bool(b) => if b { 0xffff_ffff } else { 0 },
                    SoundCoreParamValue::U32(u) => u,
                    SoundCoreParamValue::I32(i) => mem::transmute(i),
                    _ => panic!("tried to set parameter with nothing"),
                },
            };
            info!(
                self.logger,
                "Setting {}.{} = {:?}",
                self.feature.description,
                self.description,
                value
            );
            (*self.core).SetParamValue(param, param_value);
        }
    }
}

pub struct SoundCoreParameterIterator<'a> {
    target: *mut ISoundCore,
    logger: &'a Logger,
    context: u32,
    feature: &'a SoundCoreFeature<'a>,
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
    type Item = SoundCoreParameter<'a>;

    fn next(&mut self) -> Option<SoundCoreParameter<'a>> {
        unsafe {
            let mut info: ParamInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching parameter .{}.{}[{}]...",
                self.context,
                self.feature.description,
                self.index
            );
            (*self.target).EnumParams(
                self.context,
                self.index,
                self.feature.id,
                &mut info as *mut ParamInfo,
            );
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
                    let description_length = info.description
                        .iter()
                        .position(|i| *i == 0)
                        .unwrap_or_else(|| info.description.len());
                    Some(SoundCoreParameter {
                        core: self.target,
                        context: self.context,
                        feature: self.feature,
                        logger: self.logger,
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
                    })
                }
            }
        }
    }
}

pub struct SoundCore<'a>(*mut ISoundCore, &'a Logger);

impl<'a> SoundCore<'a> {
    fn bind_hardware(&self, id: &str) {
        trace!(self.1, "Binding SoundCore to {}...", id);
        let mut buffer = [0; 260];
        for c in OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo {
            info_type: 0,
            info: buffer,
        };
        unsafe { (*self.0).BindHardware(&info) }
    }
    pub fn features(&self, context: u32) -> SoundCoreFeatureIterator<'a> {
        SoundCoreFeatureIterator {
            target: self.0,
            logger: self.1,
            context: context,
            index: 0,
        }
    }
}

impl<'a> Drop for SoundCore<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing SoundCore...");
            (*self.0).Release();
        }
    }
}

fn create_sound_core<'a>(
    clsid: &GUID,
    logger: &'a Logger,
) -> Result<SoundCore<'a>, SoundCoreError> {
    unsafe {
        let mut sc: *mut ISoundCore = mem::uninitialized();
        check(CoCreateInstance(
            clsid,
            ptr::null_mut(),
            CLSCTX_ALL,
            &IID_SOUND_CORE,
            &mut sc as *mut *mut ISoundCore as *mut _,
        ))?;
        Ok(SoundCore(sc, logger))
    }
}

pub fn get_sound_core<'a>(
    clsid: &GUID,
    id: &str,
    logger: &'a Logger,
) -> Result<SoundCore<'a>, SoundCoreError> {
    let core = create_sound_core(clsid, logger)?;
    core.bind_hardware(id);
    Ok(core)
}
