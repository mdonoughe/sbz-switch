//! COM API exposed by the SoundCore interface.
//!
//! This is based on the interfaces of CtSndCr.dll.

#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_snake_case)]
#![allow(unknown_lints)]
#![allow(clippy::clippy::unreadable_literal)]

use crate::hresult::Win32Error;
use std::alloc;
use std::ptr;
use std::sync::atomic::{self, AtomicUsize, Ordering};
use winapi::ctypes::c_void;
use winapi::shared::guiddef::{IsEqualIID, REFIID};
use winapi::shared::minwindef::ULONG;
use winapi::shared::ntdef::HRESULT;
use winapi::shared::winerror::{E_INVALIDARG, E_NOINTERFACE};
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::Interface;

RIDL! {#[uuid(0x6111e7c4, 0x3ea4, 0x47ed, 0xb0, 0x74, 0xc6, 0x38, 0x87, 0x52, 0x82, 0xc4)]
interface ISoundCore(ISoundCoreVtbl): IUnknown(IUnknownVtbl) {
    fn BindHardware(
        hardware_info: *const HardwareInfo,
    ) -> HRESULT,
    fn EnumContexts(
        index: u32,
        context_info: *mut ContextInfo,
    ) -> HRESULT,
    fn GetContextInfo(
        context: u32,
        context_info: *mut ContextInfo,
    ) -> HRESULT,
    fn GetContext(
        context: *mut u32,
    ) -> HRESULT,
    fn SetContext(
        context: u32,
        restore_state: u32,
    ) -> HRESULT,
    fn EnumFeatures(
        context: u32,
        index: u32,
        feature_info: *mut FeatureInfo,
    ) -> HRESULT,
    fn GetFeatureInfo(
        context: u32,
        feature: u32,
        feature_info: *mut FeatureInfo,
    ) -> HRESULT,
    fn EnumParams(
        context: u32,
        index: u32,
        feature: u32,
        param_info: *mut ParamInfo,
    ) -> HRESULT,
    fn GetParamInfo(
        param: Param,
        info: *mut ParamInfo,
    ) -> HRESULT,
    fn GetParamValue(
        param: Param,
        value: *mut ParamValue,
    ) -> HRESULT,
    fn SetParamValue(
        param: Param,
        value: ParamValue,
    ) -> HRESULT,
    fn GetParamValueEx(
        param: Param,
        paramSize: *mut u32,
        paramData: *mut u8,
    ) -> HRESULT,
    fn SetParamValueEx(
        param: Param,
        paramSize: u32,
        paramData: *const u8,
    ) -> HRESULT,
    fn ValidateParamValue(
        param: Param,
        paramValue: ParamValue,
    ) -> HRESULT,
    fn ValidateParamValueEx(
        param: Param,
        paramSize: u32,
        paramData: *const u8,
    ) -> HRESULT,
}}

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

RIDL! {#[uuid(0xf6cb394a, 0xa680, 0x45c0, 0xac, 0xd2, 0xf0, 0x59, 0x56, 0x26, 0xa3, 0xfd)]
interface IEventNotify(IEventNotifyVtbl): IUnknown(IUnknownVtbl) {
    fn RegisterEventCallback(
        eventMask: u32,
        callback: *mut ICallback,
    ) -> HRESULT,
    fn UnregisterEventCallback() -> HRESULT,
}}

RIDL! {#[uuid(0xb353c442, 0xc49d, 0x4532, 0x9e, 0x3a, 0x1b, 0x20, 0xa1, 0x82, 0xfd, 0x00)]
interface ICallback(ICallbackVtbl): IUnknown(IUnknownVtbl) {
    fn EventCallback(
        eventInfo: EventInfo,
    ) -> HRESULT,
}}

/// Describes an event that has occurred.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EventInfo {
    pub event: u32,
    pub data_or_feature_id: u32,
    pub param_id: u32,
}

#[repr(C)]
struct Callback<C>
where
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    lpVtbl: *mut ICallbackVtbl,
    vtbl: ICallbackVtbl,
    refs: AtomicUsize,
    callback: C,
}

impl ICallback {
    /// Wraps a function in an `ICallback`.
    ///
    /// `IEventNotify` allows a single `ICallback` instance to be registered
    /// for event notifications, but implementing `ICallback` requires a lot
    /// of COM glue that we shouldn't need to worry about.
    ///
    /// # Safety
    ///
    /// You must call IUnknown.Release on the returned object when you are done
    /// with it.
    #[allow(clippy::new_ret_no_self)]
    pub unsafe fn new<C>(callback: C) -> *mut Self
    where
        C: Send + 'static + FnMut(&EventInfo) -> Result<(), Win32Error>,
    {
        let mut value = Box::new(Callback::<C> {
            lpVtbl: ptr::null_mut(),
            vtbl: ICallbackVtbl {
                parent: IUnknownVtbl {
                    QueryInterface: callback_query_interface::<C>,
                    AddRef: callback_add_ref::<C>,
                    Release: callback_release::<C>,
                },
                EventCallback: callback_event_callback::<C>,
            },
            refs: AtomicUsize::new(1),
            callback,
        });
        value.lpVtbl = &mut value.vtbl as *mut _;
        Box::into_raw(value) as *mut Self
    }
}

// ensures `this` is an instance of the expected type
unsafe fn validate<I, C>(this: *mut I) -> Result<*mut Callback<C>, Win32Error>
where
    I: Interface,
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    let this = this as *mut IUnknown;
    if this.is_null()
        || (*this).lpVtbl.is_null()
        || (*(*this).lpVtbl).QueryInterface as usize != callback_query_interface::<C> as usize
    {
        Err(Win32Error::new(E_INVALIDARG))
    } else {
        Ok(this as *mut Callback<C>)
    }
}

// converts a `Result` to an `HRESULT` so `?` can be used
unsafe fn uncheck<E>(result: E) -> HRESULT
where
    E: FnOnce() -> Result<HRESULT, Win32Error>,
{
    match result() {
        Ok(result) => result,
        Err(Win32Error { code, .. }) => code,
    }
}

unsafe extern "system" fn callback_query_interface<C>(
    this: *mut IUnknown,
    iid: REFIID,
    object: *mut *mut c_void,
) -> HRESULT
where
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    uncheck(|| {
        let this = validate::<_, C>(this)?;
        let iid = iid.as_ref().unwrap();
        if IsEqualIID(iid, &IUnknown::uuidof()) || IsEqualIID(iid, &ICallback::uuidof()) {
            (*this).refs.fetch_add(1, Ordering::Relaxed);
            *object = this as *mut c_void;
            Ok(0)
        } else {
            *object = ptr::null_mut();
            Err(Win32Error::new(E_NOINTERFACE))
        }
    })
}

unsafe extern "system" fn callback_add_ref<C>(this: *mut IUnknown) -> ULONG
where
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    match validate::<_, C>(this) {
        Ok(this) => {
            let count = (*this).refs.fetch_add(1, Ordering::Relaxed) + 1;
            count as ULONG
        }
        Err(_) => 1,
    }
}

unsafe extern "system" fn callback_release<C>(this: *mut IUnknown) -> ULONG
where
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    match validate::<_, C>(this) {
        Ok(this) => {
            let count = (*this).refs.fetch_sub(1, Ordering::Release) - 1;
            if count == 0 {
                atomic::fence(Ordering::Acquire);
                ptr::drop_in_place(this);
                alloc::dealloc(
                    this as *mut u8,
                    alloc::Layout::for_value(this.as_ref().unwrap()),
                );
            }
            count as ULONG
        }
        Err(_) => 1,
    }
}

unsafe extern "system" fn callback_event_callback<C>(
    this: *mut ICallback,
    event_info: EventInfo,
) -> HRESULT
where
    C: FnMut(&EventInfo) -> Result<(), Win32Error>,
{
    uncheck(|| {
        let this = validate::<_, C>(this)?;
        ((*this).callback)(&event_info)?;
        Ok(0)
    })
}
