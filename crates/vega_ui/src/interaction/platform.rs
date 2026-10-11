use std::rc::Rc;

use gpui_kit::{App, Context, Global, Window};

struct SystemMotionReader(Rc<dyn Fn() -> Option<bool>>);

impl Global for SystemMotionReader {}

pub fn install_system_motion(cx: &mut App) {
    install_with_reader(cx, Rc::new(reduce_motion));
}

pub(super) fn install_with_reader(cx: &mut App, read: Rc<dyn Fn() -> Option<bool>>) {
    cx.set_global(SystemMotionReader(read));
    refresh_system_motion(cx);
}

pub fn observe_system_motion<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
    cx.observe_window_activation(window, |_, window, cx| {
        if window.is_window_active() {
            refresh_system_motion(cx);
        }
    })
    .detach();
}

pub fn refresh_system_motion(cx: &mut App) {
    if let Some(reader) = cx.try_global::<SystemMotionReader>() {
        let reduce_motion = (reader.0)().unwrap_or(true);
        cx.set_reduce_motion(reduce_motion);
    }
}

#[cfg(target_os = "macos")]
fn reduce_motion() -> Option<bool> {
    use std::ffi::{c_char, c_void};

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn class_getInstanceMethod(class: *mut c_void, selector: *const c_void) -> *mut c_void;
        fn objc_msgSend();
    }
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    // SAFETY: Called only on GPUI's main thread; selectors use the documented zero-argument NSWorkspace object and BOOL signatures, and returned singleton ownership is unchanged.
    unsafe {
        let workspace_class = objc_getClass(c"NSWorkspace".as_ptr());
        let shared = sel_registerName(c"sharedWorkspace".as_ptr());
        let reduced = sel_registerName(c"accessibilityDisplayShouldReduceMotion".as_ptr());
        if workspace_class.is_null()
            || shared.is_null()
            || reduced.is_null()
            || class_getInstanceMethod(workspace_class, reduced).is_null()
        {
            return None;
        }
        let send_object: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_bool: unsafe extern "C" fn(*mut c_void, *const c_void) -> i8 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let workspace = send_object(workspace_class, shared);
        if workspace.is_null() {
            None
        } else {
            Some(send_bool(workspace, reduced) != 0)
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn reduce_motion() -> Option<bool> {
    None
}
