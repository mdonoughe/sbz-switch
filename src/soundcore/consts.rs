#![allow(missing_docs)]
#![allow(unknown_lints)]
#![allow(clippy::unreadable_literal)]

use windows::{core::GUID, Win32::UI::Shell::PropertiesSystem::PROPERTYKEY};

pub const PKEY_SOUNDCORECTL_CLSID_Z: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xc949c6aa_132b_4511_bb1b_35261a2a6333),
    pid: 0,
};
pub const PKEY_SOUNDCORECTL_CLSID_AE5: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd8570091_af3f_4615_9faa_a24845d10936),
    pid: 0,
};
