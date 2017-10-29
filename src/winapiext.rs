// This is backported from winapi 0.3.

use winapi::*;

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
pub const STGM_READ: DWORD = 0x00000000;
pub const STGM_WRITE: DWORD = 0x00000001;
pub const STGM_READWRITE: DWORD = 0x00000002;
