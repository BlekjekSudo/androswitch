//! Engine-owned table handle extracted from RustyDroid's module_tables.
//! Clone the handle before guest callbacks; never hold a RefCell borrow across
//! reentry. Handles cannot outlive their owning VM through a global registry.
use alloc::rc::Rc;
pub(crate) struct Local<T>(Rc<T>);
impl<T> Clone for Local<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Local<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
    pub(crate) fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0)
    }
}
