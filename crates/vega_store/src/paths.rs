//! Zero-dependency filesystem layout (tech-spec §6, 2026-08-29 human
//! decision — Zed-style hybrid):
//!
//! - **config root**: `${XDG_CONFIG_HOME:-$HOME/.config}/vega` on every
//!   platform; `config.toml` lives directly inside it.
//! - **data root**: macOS `$HOME/Library/Application Support/ai.vega`
//!   (bundle-id namespace); every
//!   other platform (Linux, Phase 4) `${XDG_DATA_HOME:-$HOME/.local/share}/vega`.
//!   `vega.db` lives directly inside it.
//!
//! The core resolvers are pure functions over the XDG environment value and
//! the home directory ([`config_dir_from`]/[`data_dir_from`]) so tests never
//! touch process environment variables; [`config_dir`]/[`data_dir`] are thin
//! wrappers that read the real environment. XDG values are honored only when
//! non-empty and absolute (XDG base-directory spec: relative values are
//! invalid and ignored).
//!
//! There is **no** automatic migration from the legacy `$HOME/.vega/` layout
//! (pre-release, no real users; tech-spec §6).

use std::path::{Path, PathBuf};

/// Resolves the config root from an `XDG_CONFIG_HOME` value and a home dir.
///
/// A non-empty absolute `XDG_CONFIG_HOME` wins; otherwise `$home/.config/vega`.
pub fn config_dir_from(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    match valid_xdg(xdg_config_home) {
        Some(dir) => dir.join("vega"),
        None => home.join(".config").join("vega"),
    }
}

/// Resolves the data root from an `XDG_DATA_HOME` value and a home dir.
///
/// macOS uses the bundle-id namespace
/// `$home/Library/Application Support/ai.vega` regardless of the XDG variable
/// (tech-spec §6); every other platform behaves like [`config_dir_from`] but
/// over `XDG_DATA_HOME` and `$home/.local/share/vega`.
pub fn data_dir_from(xdg_data_home: Option<&str>, home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        return home
            .join("Library")
            .join("Application Support")
            .join("ai.vega");
    }
    match valid_xdg(xdg_data_home) {
        Some(dir) => dir.join("vega"),
        None => home.join(".local").join("share").join("vega"),
    }
}

/// Config root ([`config_dir_from`]) against the process environment.
///
/// `None` when `HOME` is unset or empty.
pub fn config_dir() -> Option<PathBuf> {
    let home = home_dir()?;
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    Some(config_dir_from(xdg.as_deref(), &home))
}

/// Data root ([`data_dir_from`]) against the process environment.
///
/// `None` when `HOME` is unset or empty.
pub fn data_dir() -> Option<PathBuf> {
    let home = home_dir()?;
    let xdg = std::env::var("XDG_DATA_HOME").ok();
    Some(data_dir_from(xdg.as_deref(), &home))
}

/// The `HOME` environment variable as a path; `None` when unset or empty.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| !home.as_os_str().is_empty())
}

/// An XDG base-directory value is honored only when it is a non-empty
/// absolute path (the spec says relative values are invalid and ignored).
fn valid_xdg(value: Option<&str>) -> Option<&Path> {
    let value = value?.trim();
    let path = Path::new(value);
    (!value.is_empty() && path.is_absolute()).then_some(path)
}
