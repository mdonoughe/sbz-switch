// this is based on the interfaces of CtSndCr.dll
#![allow(dead_code)]
#![allow(non_snake_case)]

use winapi::GUID;

#[allow(unknown_lints, unreadable_literal)]
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
        context_info: *mut ContextInfo
    ) -> (),
    fn GetContextInfo(
        &mut self,
        context: u32,
        context_info: *mut ContextInfo
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
        feature_info: *mut FeatureInfo
    ) -> (),
    fn GetFeatureInfo(
        &mut self,
        context: u32,
        feature: u32,
        feature_info: *mut FeatureInfo
    ) -> (),
    fn EnumParams(
        &mut self,
        context: u32,
        index: u32,
        feature: u32,
        param_info: *mut ParamInfo
    ) -> (),
    fn GetParamInfo(
        &mut self,
        param: Param,
        info: *mut ParamInfo
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
#[derive(Debug)]
pub struct Param {
    pub param: u32,
    pub feature: u32,
    pub context: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct ParamValue {
    pub kind: u32,
    pub value: u32,
}

#[repr(C)]
pub struct HardwareInfo {
    pub info_type: u32,
    pub info: [u16; 260],
}

#[repr(C)]
#[derive(Debug)]
pub struct ContextInfo {
    pub context_id: u32,
    pub description: [u8; 32],
}

#[repr(C)]
#[derive(Debug)]
pub struct FeatureInfo {
    pub feature_id: u32,
    pub description: [u8; 32],
    pub version: [u8; 16],
}

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
