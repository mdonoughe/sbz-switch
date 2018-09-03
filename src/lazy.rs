use std::cell::UnsafeCell;

// this is like lazy_init, but not Send or Sync
pub(crate) struct Lazy<T> {
    inner: UnsafeCell<Option<T>>,
}

impl<T> Lazy<T> {
    pub fn new() -> Self {
        Lazy {
            inner: UnsafeCell::new(None),
        }
    }

    pub fn get_or_create<C: FnOnce() -> T>(&self, create: C) -> &T {
        unsafe {
            if let Some(ref value) = *self.inner.get() {
                return value;
            }
            *self.inner.get() = Some(create());
            (*self.inner.get()).as_ref().unwrap()
        }
    }
}
