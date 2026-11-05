//! Borrowed native title copied before the next VT mutation.
use crate::{Vt, ffi, vt::Error};
impl Vt {
    pub fn title(&self) -> Result<String, Error> {
        let mut title = ffi::StringView {
            ptr: std::ptr::null(),
            len: 0,
        };
        let rc = unsafe {
            ffi::ghostty_terminal_get(self.raw, ffi::DATA_TITLE, (&raw mut title).cast())
        };
        if rc != ffi::SUCCESS {
            return Err(Error::Ghostty(rc));
        }
        if title.len == 0 {
            return Ok(String::new());
        }
        // Native borrowed pointer stays valid until the next mutating call.
        let bytes = unsafe { std::slice::from_raw_parts(title.ptr, title.len) };
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}
