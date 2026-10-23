use super::placement::Rect;

#[cfg(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
mod macos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeError {
    Unsupported,
    NotMainThread,
    NativeUnavailable,
    EnumerationLimit,
    InvalidGeometry,
    InvalidMarker,
    PanelNotFound,
    PanelAmbiguous,
    NotPanel,
    PanelConfigurationRejected,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Screen {
    pub(super) id: u64,
    pub(super) frame: Rect,
    pub(super) visible: Rect,
    pub(super) appkit_frame: Rect,
}

#[derive(Debug)]
pub(super) struct DesktopGeometry {
    pub(super) settings: Option<Rect>,
    pub(super) screens: Vec<Screen>,
}

pub(super) fn geometry() -> Result<DesktopGeometry, NativeError> {
    #[cfg(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        macos::geometry()
    }
    #[cfg(not(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )))]
    {
        Err(NativeError::Unsupported)
    }
}

pub(super) fn configure_panel(marker: &str) -> Result<(), NativeError> {
    #[cfg(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        macos::configure_panel(marker)
    }
    #[cfg(not(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )))]
    {
        let _ = marker;
        Err(NativeError::Unsupported)
    }
}

pub(super) fn move_panel(
    marker: &str,
    screen: &Screen,
    origin: (f64, f64),
) -> Result<(), NativeError> {
    #[cfg(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        macos::move_panel(marker, screen, origin)
    }
    #[cfg(not(all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )))]
    {
        let _ = (marker, screen, origin);
        Err(NativeError::Unsupported)
    }
}
