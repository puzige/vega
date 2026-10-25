use super::{ImageApplication, ImageOpenError};
use std::ffi::{CStr, CString, c_char, c_void};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

type Object = *mut c_void;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Object;
    fn sel_registerName(name: *const c_char) -> Object;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> Object;
    fn objc_autoreleasePoolPop(pool: Object);
}

pub(super) fn applications(path: &Path) -> Result<Vec<ImageApplication>, ImageOpenError> {
    let path =
        CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| ImageOpenError::File)?;
    // SAFETY: AppKit objects remain inside this synchronous autorelease pool and only copied Rust data escapes.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_arg: unsafe extern "C" fn(Object, Object, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_index: unsafe extern "C" fn(Object, Object, usize) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let count: unsafe extern "C" fn(Object, Object) -> usize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let file_url: unsafe extern "C" fn(Object, Object, *const c_char, i8, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let responds: unsafe extern "C" fn(Object, Object, Object) -> i8 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let utf8: unsafe extern "C" fn(Object, Object) -> *const c_char =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let selector = |name: &'static CStr| sel_registerName(name.as_ptr());
        let pool = objc_autoreleasePoolPush();
        if pool.is_null() {
            return Err(ImageOpenError::Applications);
        }
        let result = (|| {
            let url = file_url(
                objc_getClass(c"NSURL".as_ptr()),
                selector(c"fileURLWithFileSystemRepresentation:isDirectory:relativeToURL:"),
                path.as_ptr(),
                0,
                std::ptr::null_mut(),
            );
            let workspace = send(
                objc_getClass(c"NSWorkspace".as_ptr()),
                selector(c"sharedWorkspace"),
            );
            if url.is_null() || workspace.is_null() {
                return Err(ImageOpenError::Applications);
            }
            if responds(
                workspace,
                selector(c"respondsToSelector:"),
                selector(c"URLsForApplicationsToOpenURL:"),
            ) == 0
            {
                return Err(ImageOpenError::Applications);
            }
            let urls = send_arg(workspace, selector(c"URLsForApplicationsToOpenURL:"), url);
            if urls.is_null() {
                return Err(ImageOpenError::Applications);
            }
            let default = send_arg(workspace, selector(c"URLForApplicationToOpenURL:"), url);
            let read_string = |value: Object| -> Option<String> {
                if value.is_null() {
                    return None;
                }
                let text = utf8(value, selector(c"UTF8String"));
                if text.is_null() {
                    return None;
                }
                Some(CStr::from_ptr(text).to_string_lossy().into_owned())
            };
            let read_path = |url: Object| -> Option<std::path::PathBuf> {
                if url.is_null() {
                    return None;
                }
                let bytes = utf8(url, selector(c"fileSystemRepresentation"));
                if bytes.is_null() {
                    return None;
                }
                Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(
                    CStr::from_ptr(bytes).to_bytes().to_vec(),
                )))
            };
            let default_path = read_path(default);
            let manager = send(
                objc_getClass(c"NSFileManager".as_ptr()),
                selector(c"defaultManager"),
            );
            let mut applications = Vec::new();
            let length = count(urls, selector(c"count"));
            if length > 1024 {
                return Err(ImageOpenError::Applications);
            }
            for index in 0..length {
                let app = send_index(urls, selector(c"objectAtIndex:"), index);
                let object_path = send(app, selector(c"path"));
                let Some(bundle) = read_path(app) else {
                    continue;
                };
                if !bundle.is_absolute()
                    || !bundle.is_dir()
                    || applications
                        .iter()
                        .any(|existing: &ImageApplication| existing.path == bundle)
                {
                    continue;
                }
                let Some(label) = read_string(send_arg(
                    manager,
                    selector(c"displayNameAtPath:"),
                    object_path,
                )) else {
                    continue;
                };
                let label = label
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .collect::<String>();
                if label.is_empty() {
                    continue;
                }
                applications.push(ImageApplication {
                    label,
                    is_default: default_path.as_ref() == Some(&bundle),
                    path: bundle,
                });
            }
            Ok(applications)
        })();
        objc_autoreleasePoolPop(pool);
        result
    }
}
