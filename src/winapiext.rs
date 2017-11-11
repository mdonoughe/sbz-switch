// This is backported from winapi 0.3.
#![allow(dead_code)]
#![allow(non_snake_case)]

use winapi::{DWORD, GUID, HRESULT, VARTYPE, WORD};

#[repr(C)]
#[derive(Debug)]
pub struct PROPVARIANT {
    pub vt: VARTYPE,
    wReserved1: WORD,
    wReserved2: WORD,
    wReserved3: WORD,
    pub data: [u8; 16],
}
pub type REFPROPVARIANT = *const PROPVARIANT;

RIDL!(
interface IPropertyStore(IPropertyStoreVtbl): IUnknown(IUnknownVtbl) {
    fn GetCount(
        &mut self,
        cProps: *mut DWORD
    ) -> HRESULT,
    fn GetAt(
        &mut self,
        iProp: DWORD,
        pkey: *mut PROPERTYKEY
    ) -> HRESULT,
    fn GetValue(
        &mut self,
        key: REFPROPERTYKEY,
        pv: *mut PROPVARIANT
    ) -> HRESULT,
    fn SetValue(
        &mut self,
        key: REFPROPERTYKEY,
        propvar: REFPROPVARIANT
    ) -> HRESULT,
    fn Commit(&mut self) -> HRESULT
}
);
#[repr(C)]
pub struct PROPERTYKEY {
    pub fmtid: GUID,
    pub pid: DWORD,
}
pub type REFPROPERTYKEY = *const PROPERTYKEY;
pub const STGM_READ: DWORD = 0x0;
pub const STGM_WRITE: DWORD = 0x1;
pub const STGM_READWRITE: DWORD = 0x2;

// This is missing from winapi.
#[allow(unknown_lints, unreadable_literal)]
pub const IID_AUDIO_ENDPOINT_VOLUME: GUID = GUID {
    Data1: 0x5cdf2c82,
    Data2: 0x841e,
    Data3: 0x4546,
    Data4: [0x97, 0x22, 0x0c, 0xf7, 0x40, 0x78, 0x22, 0x9a],
};

RIDL!(
interface IAudioEndpointVolume(IAudioEndpointVolumeVtbl): IUnknown(IUnknownVtbl) {
    fn RegisterControlChangeNotify(
        &mut self,
        notify: *const ()
    ) -> HRESULT,
    fn UnregisterControlChangeNotify(
        &mut self,
        notify: *const ()
    ) -> HRESULT,
    fn GetChannelCount(
        &mut self,
        channel_count: *mut u32
    ) -> HRESULT,
    fn SetMasterVolumeLevel(
        &mut self,
        level_db: f32,
        event_context: *const GUID
    ) -> HRESULT,
    fn SetMasterVolumeLevelScalar(
        &mut self,
        level: f32,
        event_context: *const GUID
    ) -> HRESULT,
    fn GetMasterVolumeLevel(
        &mut self,
        level_db: *mut f32
    ) -> HRESULT,
    fn GetMasterVolumeLevelScalar(
        &mut self,
        level: *mut f32
    ) -> HRESULT,
    fn SetChannelVolumeLevel(
        &mut self,
        channel: u32,
        level_db: f32,
        event_context: *const GUID
    ) -> HRESULT,
    fn SetChannelVolumeLevelScalar(
        &mut self,
        channel: u32,
        level: f32,
        event_context: *const GUID
    ) -> HRESULT,
    fn GetChannelVolumeLevel(
        &mut self,
        channel: u32,
        level_db: *mut f32
    ) -> HRESULT,
    fn GetChannelVolumeLevelScalar(
        &mut self,
        channel: u32,
        level: *mut f32
    ) -> HRESULT,
    fn SetMute(
        &mut self,
        mute: bool,
        event_context: *const GUID
    ) -> HRESULT,
    fn GetMute(
        &mut self,
        mute: *mut bool
    ) -> HRESULT,
    fn GetVolumeStepInfo(
        &mut self,
        step: *mut u32,
        step_count: *mut u32
    ) -> HRESULT,
    fn VolumeStepUp(
        &mut self,
        event_context: *const GUID
    ) -> HRESULT,
    fn VolumeStepDown(
        &mut self,
        event_context: *const GUID
    ) -> HRESULT,
    fn QueryHardwareSupport(
        &mut self,
        hardware_support_mask: *mut DWORD
    ) -> HRESULT,
    fn GetVolumeRange(
        &mut self,
        level_min_db: *mut f32,
        level_max_db: *mut f32,
        volume_increment_db: *mut f32
    ) -> HRESULT
}
);
