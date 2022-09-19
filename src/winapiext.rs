#![allow(non_upper_case_globals)]
#![allow(unknown_lints)]
#![allow(clippy::unreadable_literal)]

use windows::{core::GUID, Win32::UI::Shell::PropertiesSystem::PROPERTYKEY};

pub const PKEY_DeviceInterface_FriendlyName: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x026e516e_b814_414b_83cd_856d6fef4822),
    pid: 2,
};
pub const PKEY_Device_DeviceDesc: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 2,
};
