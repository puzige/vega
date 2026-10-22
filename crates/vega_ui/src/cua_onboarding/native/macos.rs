use super::{DesktopGeometry, NativeError, Rect, Screen};
use std::ffi::{CStr, c_char, c_void};

type Object = *mut c_void;
type CfType = *const c_void;

#[cfg(target_arch = "aarch64")]
type ObjcBool = bool;
#[cfg(target_arch = "x86_64")]
type ObjcBool = i8;

const MAX_SCREENS: usize = 128;
const MAX_APPLICATIONS: usize = 256;
const MAX_WINDOWS: usize = 16_384;
const MAX_MARKER_BYTES: usize = 1024;
const NONACTIVATING_PANEL: usize = 1 << 7;
const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const FLOATING_LEVEL: isize = 3;
const HIDDEN_TITLE: isize = 1;
const ON_SCREEN_ONLY: u32 = 1;
const CF_NUMBER_SINT64: isize = 4;
const CF_STRING_UTF8: u32 = 0x0800_0100;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

impl From<NSRect> for Rect {
    fn from(value: NSRect) -> Self {
        Self::new(
            value.origin.x,
            value.origin.y,
            value.size.width,
            value.size.height,
        )
    }
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSApp: Object;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Object;
    fn sel_registerName(name: *const c_char) -> Object;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_autoreleasePoolPush() -> Object;
    fn objc_autoreleasePoolPop(pool: Object);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(options: u32, relative_to: u32) -> CfType;
    fn CGDisplayBounds(display: u32) -> NSRect;
    fn CGRectMakeWithDictionaryRepresentation(dictionary: CfType, rect: *mut NSRect) -> bool;
    static kCGWindowOwnerPID: CfType;
    static kCGWindowLayer: CfType;
    static kCGWindowBounds: CfType;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: CfType);
    fn CFGetTypeID(value: CfType) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: CfType) -> isize;
    fn CFArrayGetValueAtIndex(array: CfType, index: isize) -> CfType;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetValue(dictionary: CfType, key: CfType) -> CfType;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfType, number_type: isize, value: *mut c_void) -> u8;
    fn CFStringCreateWithBytes(
        allocator: CfType,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external: u8,
    ) -> CfType;
}

struct AutoreleasePool(Object);

impl AutoreleasePool {
    fn new() -> Result<Self, NativeError> {
        // SAFETY: NSThread's class query precedes AppKit access, and the pool is local to this synchronous call.
        unsafe {
            let thread = objc_getClass(c"NSThread".as_ptr());
            if thread.is_null() {
                return Err(NativeError::Unsupported);
            }
            if !send_bool(thread, c"isMainThread") {
                return Err(NativeError::NotMainThread);
            }
            let pool = objc_autoreleasePoolPush();
            if pool.is_null() {
                return Err(NativeError::NativeUnavailable);
            }
            Ok(Self(pool))
        }
    }
}

impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        // SAFETY: This non-null pool token is popped once on the same thread that created it.
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

struct OwnedCf(CfType);

impl OwnedCf {
    fn string(value: &str) -> Result<Self, NativeError> {
        let length = isize::try_from(value.len()).map_err(|_| NativeError::NativeUnavailable)?;
        // SAFETY: CFStringCreate copies these valid UTF-8 bytes and returns an owned reference or null.
        let string = unsafe {
            CFStringCreateWithBytes(std::ptr::null(), value.as_ptr(), length, CF_STRING_UTF8, 0)
        };
        if string.is_null() {
            return Err(NativeError::NativeUnavailable);
        }
        Ok(Self(string))
    }

    fn object(&self) -> Object {
        self.0.cast_mut()
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        // SAFETY: Every instance owns one non-null Core Foundation Create or Copy reference.
        unsafe { CFRelease(self.0) }
    }
}

pub(super) fn geometry() -> Result<DesktopGeometry, NativeError> {
    let _pool = AutoreleasePool::new()?;
    // SAFETY: All native objects stay on the main thread within this pool; only copied geometry escapes.
    unsafe {
        Ok(DesktopGeometry {
            screens: screens()?,
            settings: settings_window()?,
        })
    }
}

pub(super) fn configure_panel(marker: &str) -> Result<(), NativeError> {
    let _pool = AutoreleasePool::new()?;
    // SAFETY: Lookup proves an exact unique owned NSPanel before any mutation, inside the main-thread pool.
    unsafe {
        let panel = find_panel(marker)?;
        let style = send_unsigned(panel, c"styleMask");
        let behavior = send_unsigned(panel, c"collectionBehavior");
        let required_style = style | NONACTIVATING_PANEL;
        let required_behavior = behavior | CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY;
        if style != required_style {
            set_unsigned(panel, c"setStyleMask:", required_style);
        }
        if send_signed(panel, c"level") != FLOATING_LEVEL {
            set_signed(panel, c"setLevel:", FLOATING_LEVEL);
        }
        if behavior != required_behavior {
            set_unsigned(panel, c"setCollectionBehavior:", required_behavior);
        }
        if send_signed(panel, c"titleVisibility") != HIDDEN_TITLE {
            set_signed(panel, c"setTitleVisibility:", HIDDEN_TITLE);
        }
        if send_unsigned(panel, c"styleMask") != required_style
            || send_unsigned(panel, c"collectionBehavior") != required_behavior
            || send_signed(panel, c"level") != FLOATING_LEVEL
            || send_signed(panel, c"titleVisibility") != HIDDEN_TITLE
        {
            return Err(NativeError::PanelConfigurationRejected);
        }
        Ok(())
    }
}

pub(super) fn move_panel(
    marker: &str,
    screen: &Screen,
    origin: (f64, f64),
) -> Result<(), NativeError> {
    let _pool = AutoreleasePool::new()?;
    if screen.id == 0
        || u32::try_from(screen.id).is_err()
        || !valid_rect(screen.frame)
        || !valid_rect(screen.appkit_frame)
        || !valid_rect(screen.visible)
        || !origin.0.is_finite()
        || !origin.1.is_finite()
    {
        return Err(NativeError::InvalidGeometry);
    }
    // SAFETY: The unique owned panel and its current frame are used only inside this main-thread pool.
    unsafe {
        let panel = find_panel(marker)?;
        let current = Rect::from(send_rect(panel, c"frame"));
        if !valid_rect(current) {
            return Err(NativeError::InvalidGeometry);
        }
        let point = NSPoint {
            x: screen.appkit_frame.x + origin.0 - screen.frame.x,
            y: screen.appkit_frame.y + screen.appkit_frame.height
                - (origin.1 - screen.frame.y)
                - current.height,
        };
        if !valid_rect(Rect::new(point.x, point.y, current.width, current.height)) {
            return Err(NativeError::InvalidGeometry);
        }
        set_origin(panel, point);
        Ok(())
    }
}

unsafe fn screens() -> Result<Vec<Screen>, NativeError> {
    // SAFETY: The caller holds a main-thread pool; NSScreen supplies typed objects and copied frames.
    unsafe {
        let array = send_object(class(c"NSScreen")?, c"screens");
        let count = array_count(array, MAX_SCREENS)?;
        if count == 0 {
            return Err(NativeError::InvalidGeometry);
        }
        let key = OwnedCf::string("NSScreenNumber")?;
        let mut screens = Vec::with_capacity(count);
        for index in 0..count {
            let screen = required_object(send_index(array, index))?;
            let description = required_object(send_object(screen, c"deviceDescription"))?;
            let number = send_object_arg(description, c"objectForKey:", key.object());
            let id = cf_integer(number.cast_const())
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id != 0)
                .ok_or(NativeError::InvalidGeometry)?;
            let frame = Rect::from(CGDisplayBounds(id));
            let appkit_frame = Rect::from(send_rect(screen, c"frame"));
            let appkit_visible = Rect::from(send_rect(screen, c"visibleFrame"));
            if !valid_rect(frame) || !valid_rect(appkit_frame) || !valid_rect(appkit_visible) {
                return Err(NativeError::InvalidGeometry);
            }
            let visible = Rect::new(
                frame.x + appkit_visible.x - appkit_frame.x,
                frame.y + appkit_frame.height
                    - (appkit_visible.y - appkit_frame.y)
                    - appkit_visible.height,
                appkit_visible.width,
                appkit_visible.height,
            );
            if !valid_rect(visible)
                || screens
                    .iter()
                    .any(|screen: &Screen| screen.id == u64::from(id))
            {
                return Err(NativeError::InvalidGeometry);
            }
            screens.push(Screen {
                id: u64::from(id),
                frame,
                visible,
                appkit_frame,
            });
        }
        Ok(screens)
    }
}

unsafe fn settings_window() -> Result<Option<Rect>, NativeError> {
    // SAFETY: AppKit is main-thread scoped, CF values are type-checked, and the Copy array is released by RAII.
    unsafe {
        let bundle = OwnedCf::string("com.apple.systempreferences")?;
        let applications = send_object_arg(
            class(c"NSRunningApplication")?,
            c"runningApplicationsWithBundleIdentifier:",
            bundle.object(),
        );
        if applications.is_null() {
            return Ok(None);
        }
        let count = array_count(applications, MAX_APPLICATIONS)?;
        let mut pids = Vec::with_capacity(count);
        for index in 0..count {
            let application = required_object(send_index(applications, index))?;
            let pid = send_pid(application);
            if pid > 0 {
                pids.push(i64::from(pid));
            }
        }
        if pids.is_empty() {
            return Ok(None);
        }
        let windows = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY, 0);
        if windows.is_null() {
            return Ok(None);
        }
        let windows = OwnedCf(windows);
        if CFGetTypeID(windows.0) != CFArrayGetTypeID() {
            return Err(NativeError::NativeUnavailable);
        }
        let count = CFArrayGetCount(windows.0);
        let length = usize::try_from(count).map_err(|_| NativeError::EnumerationLimit)?;
        if length > MAX_WINDOWS {
            return Err(NativeError::EnumerationLimit);
        }
        let mut largest: Option<Rect> = None;
        for index in 0..count {
            let window = CFArrayGetValueAtIndex(windows.0, index);
            if window.is_null() || CFGetTypeID(window) != CFDictionaryGetTypeID() {
                continue;
            }
            let Some(pid) = cf_integer(CFDictionaryGetValue(window, kCGWindowOwnerPID)) else {
                continue;
            };
            if !pids.contains(&pid)
                || cf_integer(CFDictionaryGetValue(window, kCGWindowLayer)) != Some(0)
            {
                continue;
            }
            let bounds = CFDictionaryGetValue(window, kCGWindowBounds);
            if bounds.is_null() || CFGetTypeID(bounds) != CFDictionaryGetTypeID() {
                continue;
            }
            let mut bounds_rect = NSRect::default();
            if !CGRectMakeWithDictionaryRepresentation(bounds, &mut bounds_rect) {
                continue;
            }
            let rect = Rect::from(bounds_rect);
            if valid_rect(rect) && largest.is_none_or(|largest| rect.area() > largest.area()) {
                largest = Some(rect);
            }
        }
        Ok(largest)
    }
}

unsafe fn find_panel(marker: &str) -> Result<Object, NativeError> {
    if marker.is_empty() || marker.len() > MAX_MARKER_BYTES || marker.contains('\0') {
        return Err(NativeError::InvalidMarker);
    }
    let marker = OwnedCf::string(marker)?;
    // SAFETY: NSApp owns the windows, the pool keeps the array alive, and this read-only scan runs on the main thread.
    unsafe {
        let application = required_object(NSApp)?;
        let windows = send_object(application, c"windows");
        let count = array_count(windows, MAX_WINDOWS)?;
        let mut matching = None;
        for index in 0..count {
            let window = required_object(send_index(windows, index))?;
            let title = send_object(window, c"title");
            if !title.is_null() && send_bool_arg(title, c"isEqualToString:", marker.object()) {
                if matching.is_some() {
                    return Err(NativeError::PanelAmbiguous);
                }
                matching = Some(window);
            }
        }
        let panel = matching.ok_or(NativeError::PanelNotFound)?;
        if !send_bool_arg(panel, c"isKindOfClass:", class(c"NSPanel")?) {
            return Err(NativeError::NotPanel);
        }
        Ok(panel)
    }
}

fn valid_rect(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
        && rect.max_x().is_finite()
        && rect.max_y().is_finite()
        && rect.area().is_finite()
        && rect.area() > 0.0
}

fn required_object(object: Object) -> Result<Object, NativeError> {
    if object.is_null() {
        Err(NativeError::NativeUnavailable)
    } else {
        Ok(object)
    }
}

unsafe fn class(name: &'static CStr) -> Result<Object, NativeError> {
    // SAFETY: The class name is static and NUL-terminated; a missing class is rejected.
    unsafe { required_object(objc_getClass(name.as_ptr())) }
}

unsafe fn array_count(array: Object, limit: usize) -> Result<usize, NativeError> {
    required_object(array)?;
    // SAFETY: Callers provide a live NSArray obtained from the documented native array APIs.
    let count = unsafe { send_unsigned(array, c"count") };
    if count > limit {
        return Err(NativeError::EnumerationLimit);
    }
    Ok(count)
}

unsafe fn cf_integer(value: CfType) -> Option<i64> {
    // SAFETY: The borrowed CF or NSNumber object is checked for null and type before decoding to an i64.
    unsafe {
        if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
            return None;
        }
        let mut number = 0_i64;
        if CFNumberGetValue(value, CF_NUMBER_SINT64, (&mut number as *mut i64).cast()) == 0 {
            return None;
        }
        Some(number)
    }
}

fn bool_value(value: ObjcBool) -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        value
    }
    #[cfg(target_arch = "x86_64")]
    {
        value != 0
    }
}

unsafe fn send_object(receiver: Object, selector: &'static CStr) -> Object {
    // SAFETY: Callers supply a live receiver and a selector returning an Objective-C object without arguments.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()))
    }
}

unsafe fn send_object_arg(receiver: Object, selector: &'static CStr, argument: Object) -> Object {
    // SAFETY: The selector takes one object argument and returns an object; both inputs remain live during dispatch.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()), argument)
    }
}

unsafe fn send_index(array: Object, index: usize) -> Object {
    // SAFETY: The live NSArray and NSUInteger index are validated against its bounded count before dispatch.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, usize) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(array, sel_registerName(c"objectAtIndex:".as_ptr()), index)
    }
}

unsafe fn send_bool(receiver: Object, selector: &'static CStr) -> bool {
    // SAFETY: The selector returns BOOL, represented as bool on arm64 and signed char on x86_64.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> ObjcBool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        bool_value(send(receiver, sel_registerName(selector.as_ptr())))
    }
}

unsafe fn send_bool_arg(receiver: Object, selector: &'static CStr, argument: Object) -> bool {
    // SAFETY: The live receiver's selector takes one object argument and returns the architecture-specific BOOL.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, Object) -> ObjcBool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        bool_value(send(
            receiver,
            sel_registerName(selector.as_ptr()),
            argument,
        ))
    }
}

unsafe fn send_unsigned(receiver: Object, selector: &'static CStr) -> usize {
    // SAFETY: The selector returns NSUInteger, which is pointer-sized unsigned on the supported macOS targets.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> usize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()))
    }
}

unsafe fn send_signed(receiver: Object, selector: &'static CStr) -> isize {
    // SAFETY: The selector returns NSInteger, which is pointer-sized signed on the supported macOS targets.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> isize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()))
    }
}

unsafe fn send_pid(application: Object) -> i32 {
    // SAFETY: NSRunningApplication.processIdentifier returns pid_t, a signed 32-bit integer on macOS.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> i32 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(application, sel_registerName(c"processIdentifier".as_ptr()))
    }
}

unsafe fn set_unsigned(receiver: Object, selector: &'static CStr, value: usize) {
    // SAFETY: The owned NSPanel selector takes NSUInteger by value and returns void.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, usize) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()), value);
    }
}

unsafe fn set_signed(receiver: Object, selector: &'static CStr, value: isize) {
    // SAFETY: The owned NSPanel selector takes NSInteger by value and returns void.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, isize) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()), value);
    }
}

unsafe fn set_origin(panel: Object, point: NSPoint) {
    // SAFETY: setFrameOrigin: takes the repr(C) two-double NSPoint by value, not a pointer, and returns void.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object, NSPoint) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(panel, sel_registerName(c"setFrameOrigin:".as_ptr()), point);
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn send_rect(receiver: Object, selector: &'static CStr) -> NSRect {
    // SAFETY: On arm64 a repr(C) four-double NSRect uses ordinary objc_msgSend with its native aggregate return ABI.
    unsafe {
        let send: unsafe extern "C" fn(Object, Object) -> NSRect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr()))
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn send_rect(receiver: Object, selector: &'static CStr) -> NSRect {
    // SAFETY: On x86_64 the 32-byte NSRect uses objc_msgSend_stret with the result pointer before self and selector.
    unsafe {
        let send: unsafe extern "C" fn(*mut NSRect, Object, Object) =
            std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
        let mut rect = NSRect::default();
        send(&mut rect, receiver, sel_registerName(selector.as_ptr()));
        rect
    }
}
