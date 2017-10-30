// this is based on the interfaces of CtSndCr.dll
#![allow(dead_code)]
#![allow(non_snake_case)]

use winapi::{DWORD, GUID};

pub const IID_SOUND_CORE: GUID = GUID {
    Data1: 0x6111e7c4,
    Data2: 0x3ea4,
    Data3: 0x47ed,
    Data4: [0xb0, 0x74, 0xc6, 0x38, 0x87, 0x52, 0x82, 0xc4],
};

RIDL!(
interface ISoundCore(ISoundCoreVtbl): IUnknown(IUnknownVtbl) {
    fn BindHardware(
        &mut self,
        hardware_info: *const HardwareInfo
    ) -> (),
    fn EnumContexts(
        &mut self,
        index: u32,
        context_info: *mut ()
    ) -> (),
    fn GetContextInfo(
        &mut self,
        context: u32,
        context_info: *mut ()
    ) -> (),
    fn GetContext(
        &mut self,
        context: *mut u32
    ) -> (),
    fn SetContext(
        &mut self,
        context: u32,
        restore_state: u32
    ) -> (),
    fn EnumFeatures(
        &mut self,
        context: u32,
        index: u32,
        feature_info: *mut ()
    ) -> (),
    fn GetFeatureInfo(
        &mut self,
        context: u32,
        feature: u32,
        feature_info: *mut ()
    ) -> (),
    fn EnumParams(
        &mut self,
        context: u32,
        index: u32,
        feature: u32,
        param_info: *mut ()
    ) -> (),
    fn GetParamInfo(
        &mut self,
        param: Param,
        info: *mut ()
    ) -> (),
    fn GetParamValue(
        &mut self,
        param: Param,
        value: *mut ParamValue
    ) -> (),
    fn SetParamValue(
        &mut self,
        param: Param,
        value: ParamValue
    ) -> ()
}
);

#[repr(C)]
pub struct Param {
    pub context: u32,
    pub feature: u32,
    pub param: u32,
}

#[repr(C)]
pub struct ParamValue {
    pub kind: u32,
    pub value: u32,
}

#[repr(C)]
pub struct HardwareInfo {
    pub info_type: DWORD,
    pub info: [u16; 260],
}
