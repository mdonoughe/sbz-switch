//! COM API exposed by the SoundCore interface.
//!
//! This is based on the interfaces of CtSndCr.dll.

#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(unknown_lints)]
#![allow(clippy::clippy::unreadable_literal)]

use std::sync::Mutex;

use futures::channel::mpsc::UnboundedSender;
use futures::executor;
use futures::SinkExt;
use windows::core::implement;
use windows::core::interface;
use windows::core::IUnknown;
use windows::core::IUnknown_Vtbl;
use windows::core::HRESULT;
use windows::Win32::Foundation::E_ABORT;
use windows::Win32::Foundation::S_OK;

#[interface("6111e7c4-3ea4-47ed-b074-c638875282c4")]
pub(crate) unsafe trait ISoundCore: IUnknown {
    pub fn BindHardware(&self, hardware_info: *const HardwareInfo) -> HRESULT;
    pub fn EnumContexts(&self, index: u32, context_info: *mut ContextInfo) -> HRESULT;
    pub fn GetContextInfo(&self, context: u32, context_info: *mut ContextInfo) -> HRESULT;
    pub fn GetContext(&self, context: *mut u32) -> HRESULT;
    pub fn SetContext(&self, context: u32, restore_state: u32) -> HRESULT;
    pub fn EnumFeatures(&self, context: u32, index: u32, info: *mut FeatureInfo) -> HRESULT;
    pub fn GetFeatureInfo(&self, context: u32, feature: u32, info: *mut FeatureInfo) -> HRESULT;
    pub fn EnumParams(
        &self,
        context: u32,
        index: u32,
        feature: u32,
        info: *mut ParamInfo,
    ) -> HRESULT;
    pub fn GetParamInfo(&self, param: Param, info: *mut ParamInfo) -> HRESULT;
    pub fn GetParamValue(&self, param: Param, value: *mut ParamValue) -> HRESULT;
    pub fn SetParamValue(&self, param: Param, value: ParamValue) -> HRESULT;
    pub fn GetParamValueEx(&self, param: Param, paramSize: *mut u32, paramData: *mut u8)
        -> HRESULT;
    pub fn SetParamValueEx(&self, param: Param, paramSize: u32, paramData: *const u8) -> HRESULT;
    pub fn ValidateParamValue(&self, param: Param, paramValue: ParamValue) -> HRESULT;
    pub fn ValidateParamValueEx(
        &self,
        param: Param,
        paramSize: u32,
        paramData: *const u8,
    ) -> HRESULT;
}

/// References a parameter of a feature of a device.
#[repr(C)]
#[derive(Debug)]
pub struct Param {
    pub param: u32,
    pub feature: u32,
    pub context: u32,
}

/// Represents a value of a parameter.
#[repr(C)]
#[derive(Debug)]
pub struct ParamValue {
    pub kind: u32,
    pub value: u32,
}

/// References a hardware device.
#[repr(C)]
pub struct HardwareInfo {
    pub info_type: u32,
    pub info: [u16; 260],
}

/// Describes a context the device may operate in.
#[repr(C)]
#[derive(Debug)]
pub struct ContextInfo {
    pub context_id: u32,
    pub description: [u8; 32],
}

/// Describes a feature exposed by a device.
#[repr(C)]
#[derive(Debug)]
pub struct FeatureInfo {
    pub feature_id: u32,
    pub description: [u8; 32],
    pub version: [u8; 16],
}

/// Describes a parameter of a feature exposed by a device.
#[repr(C)]
#[derive(Debug)]
pub struct ParamInfo {
    pub param: Param,
    pub param_type: u32,
    pub data_size: u32,
    pub min_value: ParamValue,
    pub max_value: ParamValue,
    pub step_size: ParamValue,
    pub default_value: ParamValue,
    pub param_attributes: u32,
    pub description: [u8; 32],
}

#[interface("f6cb394a-a680-45c0-acd2-f0595626a3fd")]
pub(crate) unsafe trait IEventNotify: IUnknown {
    pub unsafe fn RegisterEventCallback(&self, eventMask: u32, callback: ICallback) -> HRESULT;
    pub unsafe fn UnregisterEventCallback(&self) -> HRESULT;
}

#[interface("b353c442-c49d-4532-9e3a-1b20a182fd00")]
pub(crate) unsafe trait ICallback: IUnknown {
    unsafe fn EventCallback(&self, eventInfo: EventInfo) -> HRESULT;
}

/// Describes an event that has occurred.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EventInfo {
    pub event: u32,
    pub data_or_feature_id: u32,
    pub param_id: u32,
}

#[implement(ICallback)]
pub(crate) struct Callback {
    sender: Mutex<UnboundedSender<EventInfo>>,
}

impl Callback {
    pub fn new(sender: UnboundedSender<EventInfo>) -> Self {
        Self {
            sender: Mutex::new(sender),
        }
    }
}

impl ICallback_Impl for Callback {
    unsafe fn EventCallback(&self, event_info: EventInfo) -> HRESULT {
        match executor::block_on(self.sender.lock().unwrap().send(event_info)) {
            Ok(()) => S_OK,
            Err(_) => E_ABORT,
        }
    }
}
