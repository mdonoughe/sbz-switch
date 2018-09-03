use std::fmt;
use std::ops::Deref;
use std::ptr::{null_mut, NonNull};

use winapi::um::combaseapi::{CoInitializeEx, CoUninitialize};
use winapi::um::objbase::COINIT_APARTMENTTHREADED;
use winapi::um::unknwnbase::IUnknown;

use hresult::{check, Win32Error};

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
pub fn initialize_com() -> Result<(), Win32Error> {
    unsafe { check(CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED)).and(Ok(())) }
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
    pub fn new() -> Result<Self, Win32Error> {
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
    T: Deref<Target = IUnknown>,
{
    inner: NonNull<T>,
    _scope: ComScope,
}

impl<T> ComObject<T>
where
    T: Deref<Target = IUnknown>,
{
    pub unsafe fn take(inner: *mut T) -> Self {
        Self {
            inner: NonNull::new(inner).unwrap(),
            _scope: ComScope::new().unwrap(),
        }
    }
}

impl<T> Deref for ComObject<T>
where
    T: Deref<Target = IUnknown>,
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.inner.as_ref() }
    }
}

impl<T> Clone for ComObject<T>
where
    T: Deref<Target = IUnknown>,
{
    fn clone(&self) -> Self {
        let scope = ComScope::new().unwrap();
        unsafe {
            self.inner.as_ref().AddRef();
        }
        Self {
            inner: self.inner,
            _scope: scope,
        }
    }
}

impl<T> fmt::Debug for ComObject<T>
where
    T: Deref<Target = IUnknown>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ComObject {{ {} }}", self.inner.as_ptr() as usize)
    }
}

impl<T> Drop for ComObject<T>
where
    T: Deref<Target = IUnknown>,
{
    fn drop(&mut self) {
        unsafe {
            self.inner.as_ref().Release();
        }
    }
}
