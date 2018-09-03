use std::mem;
use std::str;

use slog::Logger;

use winapi::shared::winerror::E_ACCESSDENIED;

use ctsndcr::{ISoundCore, Param, ParamInfo, ParamValue};
use hresult::{check, Win32Error};

/// A value of a parameter.
#[derive(Debug)]
pub enum SoundCoreParamValue {
    /// A floating point value
    Float(f32),
    /// A boolean value
    Bool(bool),
    /// An unsigned integer value
    U32(u32),
    /// A signed integer value
    I32(i32),
    /// No value
    None,
}

/// Represents a parameter of a feature.
#[derive(Debug)]
pub struct SoundCoreParameter {
    core: *mut ISoundCore,
    logger: Logger,
    context: u32,
    feature_id: u32,
    feature_description: String,
    /// A numeric ID for this value
    pub id: u32,
    /// The kind of the value
    pub kind: u32,
    /// The size of the value, or `None`
    pub size: Option<u32>,
    /// The minimum acceptable value, or `None`
    pub min_value: SoundCoreParamValue,
    /// The maximum acceptable value, or `None`
    pub max_value: SoundCoreParamValue,
    /// The distance between acceptable values, or `None`
    pub step_size: SoundCoreParamValue,
    /// Parameter attributes
    pub attributes: u32,
    /// A description of the parameter
    pub description: String,
}

impl SoundCoreParameter {
    pub(crate) fn new(
        core: *mut ISoundCore,
        feature_description: String,
        logger: Logger,
        info: &ParamInfo,
    ) -> Self {
        let description_length = info
            .description
            .iter()
            .position(|i| *i == 0)
            .unwrap_or_else(|| info.description.len());
        let result = Self {
            core,
            context: info.param.context,
            feature_id: info.param.feature,
            feature_description: feature_description,
            logger,
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
        };
        unsafe {
            (*core).AddRef();
        }
        result
    }
    /// Gets the value of a parameter.
    ///
    /// May return `Err(Win32Error { code: E_ACCESSDENIED })` when getting a
    /// parameter that is not currently applicable.
    pub fn get(&self) -> Result<SoundCoreParamValue, Win32Error> {
        // varsize -> not supported
        if self.kind == 5 {
            return Ok(SoundCoreParamValue::None);
        }
        unsafe {
            let param = Param {
                context: self.context,
                feature: self.feature_id,
                param: self.id,
            };
            let mut value: ParamValue = mem::uninitialized();
            trace!(
                self.logger,
                "Fetching parameter value .{}.{}.{}...",
                self.context,
                self.feature_id,
                self.id
            );
            match check((*self.core).GetParamValue(param, &mut value as *mut ParamValue)) {
                Ok(_) => {}
                Err(Win32Error { code: code @ _, .. }) if code == E_ACCESSDENIED => {
                    trace!(
                        self.logger,
                        "Got parameter value .{}.{}.{} = {}",
                        self.context,
                        self.feature_id,
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
                self.feature_id,
                self.id,
                value
            );
            Ok(convert_param_value(&value))
        }
    }
    /// Sets the value of a parameter.
    ///
    /// May return `Err(Win32Error { code: E_ACCESSDENIED })` when setting a
    /// parameter that is not currently applicable.
    pub fn set(&self, value: &SoundCoreParamValue) -> Result<(), Win32Error> {
        unsafe {
            let param = Param {
                context: self.context,
                feature: self.feature_id,
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
                "Setting {}.{} = {:?}", self.feature_description, self.description, value
            );
            check((*self.core).SetParamValue(param, param_value))?;
            Ok(())
        }
    }
}

impl Drop for SoundCoreParameter {
    fn drop(&mut self) {
        unsafe {
            (*self.core).Release();
        }
    }
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
