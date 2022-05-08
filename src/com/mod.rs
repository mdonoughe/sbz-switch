pub mod event;

use std::fmt;
use std::ops::Deref;
use std::ptr::null_mut;

use windows::core::Interface;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

/// Prepares the current thread for running COM by calling CoInitializeEx.
///
/// This initializes the current thread in single-threaded apartment mode (STA).
///
/// CoInitializeEx may be called multiple times.
///
/// # Examples
///
/// ```
/// initialize_com();
/// // do COM things
/// uninitialize_com();
/// ```
pub fn initialize_com() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) }
}

/// Unconfigures COM for the current thread by calling CoUninitialize.
///
/// This may not actually do anything if CoInitializeEx has been called multiple times.
pub fn uninitialize_com() {
    unsafe {
        CoUninitialize();
    }
}

pub(crate) struct ComScope {}

impl ComScope {
    pub fn begin() -> windows::core::Result<Self> {
        initialize_com()?;
        Ok(Self {})
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        uninitialize_com();
    }
}

pub(crate) struct ComObject<T>
where
    T: Interface,
{
    inner: T,
    _scope: ComScope,
}

impl<T> ComObject<T>
where
    T: Interface,
{
    pub unsafe fn take(inner: T) -> Self {
        Self {
            inner,
            _scope: ComScope::begin().unwrap(),
        }
    }
}

impl<T> Deref for ComObject<T>
where
    T: Interface,
{
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> Clone for ComObject<T>
where
    T: Clone + Interface,
{
    fn clone(&self) -> Self {
        let scope = ComScope::begin().unwrap();
        Self {
            inner: self.inner.clone(),
            _scope: scope,
        }
    }
}

impl<T> fmt::Debug for ComObject<T>
where
    T: fmt::Debug + Interface,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComObject")
            .field("inner", &self.inner)
            .finish()
    }
}
