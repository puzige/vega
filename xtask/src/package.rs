//! `cargo xtask package` — assemble a distributable macOS app bundle.
//!
//! S8 packaging card: `cargo build --release` (via the shared T43 rebuild
//! helper), then bundle `target/release/vega` into `dist/Vega.app` with a
//! real icon and metadata, ad-hoc codesign it, verify the signature and zip
//! it up as `dist/Vega-macos-arm64.zip` for direct copy to other Macs.
//!
//! Zero new dependencies: every step shells out to macOS built-ins
//! (`qlmanage`, `sips`, `iconutil`, `codesign`, `zip`).
//!
//! Icon source decision (recorded per the card): the app icon is rendered
//! from `assets/logo/vega-icon-f1-light.svg` (F1, 浅色主标). Rationale —
//! LOGO.md designates F1 as the primary Dock icon; the F1/F3 raster
//! originals carry the hunyuan "AI 生成" watermark in their bottom-right
//! corner and LOGO.md states the hand-vector SVG is the definitive
//! artwork, so the iconset is generated from the SVG, not the PNGs. `.icns`
//! has no automatic light/dark appearance variants, and LOGO.md's style
//! ruling (classic flat third-party squircle, cf. Telegram/VS Code) makes
//! the F1 white squircle suitable for both Dock appearances; F3 (深色变体)
//! stays a marketing asset. The iconset intermediate files live in a temp
//! directory and are never committed — the icns is reproducible from the
//! committed SVG.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::provenance;

/// Bundle identity — MUST stay `ai.vega`. It is the namespace of the macOS
/// data root `$HOME/Library/Application Support/ai.vega` and of the Keychain
/// service (see `crates/vega_store/src/paths.rs`); changing it would orphan
/// every existing dogfood profile.
const BUNDLE_ID: &str = "ai.vega";
const BUNDLE_NAME: &str = "Vega";
/// The executable keeps the crate's lowercase name (GPUI args/ps visibility).
const EXECUTABLE_NAME: &str = "vega";
/// `CFBundleIconFile` without the `.icns` extension.
const ICON_FILE: &str = "Vega";
/// arm64-only build; Apple Silicon starts at macOS 11 (GPUI/Metal needs more
/// than the zed-rev floor of 10.15.7 anyway, so 11.0 is the honest floor).
const MIN_MACOS: &str = "11.0";
const CATEGORY: &str = "public.app-category.developer-tools";
/// Workspace version (`[workspace.package] version`, inherited by xtask).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// (pixel size, iconset filename) — Apple's standard macOS app icon set:
/// 16/32/128/256/512 at @1x plus their @2x variants (up to 1024 px), the
/// exact set LOGO.md's TODO calls for.
const ICONSET_ENTRIES: &[(u32, &str)] = &[
    (16, "icon_16x16.png"),
    (32, "icon_16x16@2x.png"),
    (32, "icon_32x32.png"),
    (64, "icon_32x32@2x.png"),
    (128, "icon_128x128.png"),
    (256, "icon_128x128@2x.png"),
    (256, "icon_256x256.png"),
    (512, "icon_256x256@2x.png"),
    (512, "icon_512x512.png"),
    (1024, "icon_512x512@2x.png"),
];

const PKG_INFO: &str = "APPL????";
/// Bundle directory name inside `dist/`.
const APP_BUNDLE: &str = "Vega.app";

/// Entry point of `cargo xtask package`.
pub fn run() -> Result<()> {
    let workspace = crate::workspace_root()?;
    let build = provenance::rebuild_release(&workspace)?;

    let dist = workspace.join("dist");
    if dist.exists() {
        fs::remove_dir_all(&dist).with_context(|| format!("failed to clear {}", dist.display()))?;
    }
    let app = dist.join(format!("{BUNDLE_NAME}.app"));
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("MacOS"))
        .and_then(|()| fs::create_dir_all(contents.join("Resources")))
        .with_context(|| format!("failed to lay out {}", contents.display()))?;

    // Binary: copy as the bundle executable with 0755.
    let executable = contents.join("MacOS").join(EXECUTABLE_NAME);
    fs::copy(&build.vega_bin, &executable).with_context(|| {
        format!(
            "failed to copy {} into the bundle",
            build.vega_bin.display()
        )
    })?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to chmod {}", executable.display()))?;

    // Icon: SVG → 1024 PNG (qlmanage) → iconset (sips) → .icns (iconutil).
    let icon_path = contents.join("Resources").join(format!("{ICON_FILE}.icns"));
    render_icon(&workspace, &icon_path)?;

    // Metadata.
    fs::write(contents.join("Info.plist"), info_plist()).context("failed to write Info.plist")?;
    fs::write(contents.join("PkgInfo"), PKG_INFO).context("failed to write PkgInfo")?;

    // Ad-hoc signature + verification (no identity required; other Macs
    // still need the Gatekeeper right-click open, see INSTALL.txt/docs).
    run_tool(
        "codesign",
        &["--force", "--deep", "--sign", "-", APP_BUNDLE],
        Some(dist.as_path()),
    )?;
    run_tool(
        "codesign",
        &["--verify", "--strict", APP_BUNDLE],
        Some(dist.as_path()),
    )?;
    run_tool(
        "plutil",
        &["-lint", "Contents/Info.plist"],
        Some(app.as_path()),
    )?;

    // Distributable zip: bundle + install instructions.
    fs::write(dist.join("INSTALL.txt"), install_txt()).context("failed to write INSTALL.txt")?;
    let zip = "Vega-macos-arm64.zip";
    run_tool(
        "zip",
        &["-r", "-q", zip, APP_BUNDLE, "INSTALL.txt"],
        Some(dist.as_path()),
    )?;

    println!("\nbundle structure:");
    print_tree(&app)?;
    println!(
        "\npackaged {} (icon {}) →\n  {}/{}\n  {}",
        VERSION,
        icon_path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "Vega.icns".into()),
        dist.display(),
        zip,
        app.display(),
    );
    Ok(())
}

/// Renders the F1 light SVG into the target `.icns` (see module docs for the
/// source-choice rationale) and returns the icns path.
fn render_icon(workspace: &Path, target: &Path) -> Result<std::path::PathBuf> {
    let svg = workspace.join("assets/logo/vega-icon-f1-light.svg");
    if !svg.exists() {
        bail!("icon source not found at {}", svg.display());
    }
    let staging = std::env::temp_dir().join(format!("vega-package-iconset-{}", crate::unix_ms()));
    let iconset = staging.join("Vega.iconset");
    fs::create_dir_all(&iconset)
        .with_context(|| format!("failed to create {}", iconset.display()))?;

    // qlmanage (WebKit) rasterizes the SVG at 1024 with transparency outside
    // the squircle; sips then normalizes to an exact 1024×1024 base.
    let staging_arg = staging.to_string_lossy().into_owned();
    let svg_arg = svg.to_string_lossy().into_owned();
    run_tool(
        "qlmanage",
        &[
            "-t",
            "-s",
            "1024",
            "-o",
            staging_arg.as_str(),
            svg_arg.as_str(),
        ],
        None,
    )?;
    let rendered = staging.join("vega-icon-f1-light.svg.png");
    let base = staging.join("icon-1024.png");
    let rendered_arg = rendered.to_string_lossy().into_owned();
    let base_arg = base.to_string_lossy().into_owned();
    run_tool(
        "sips",
        &[
            "-z",
            "1024",
            "1024",
            rendered_arg.as_str(),
            "--out",
            base_arg.as_str(),
        ],
        None,
    )?;
    for (size, name) in ICONSET_ENTRIES {
        let dimension = size.to_string();
        let target_arg = iconset.join(name).to_string_lossy().into_owned();
        run_tool(
            "sips",
            &[
                "-z",
                dimension.as_str(),
                dimension.as_str(),
                base_arg.as_str(),
                "--out",
                target_arg.as_str(),
            ],
            None,
        )?;
    }
    let iconset_arg = iconset.to_string_lossy().into_owned();
    let icns_arg = target.to_string_lossy().into_owned();
    run_tool(
        "iconutil",
        &["-c", "icns", iconset_arg.as_str(), "-o", icns_arg.as_str()],
        None,
    )?;
    // Best-effort cleanup; a leftover temp dir must not fail packaging.
    let _ = fs::remove_dir_all(&staging);
    Ok(target.to_path_buf())
}

/// The Info.plist contents. All values are compile-time constants without
/// XML special characters, so plain interpolation is safe.
fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>{BUNDLE_NAME}</string>
	<key>CFBundleDisplayName</key>
	<string>{BUNDLE_NAME}</string>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_ID}</string>
	<key>CFBundleVersion</key>
	<string>{VERSION}</string>
	<key>CFBundleShortVersionString</key>
	<string>{VERSION}</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE_NAME}</string>
	<key>CFBundleIconFile</key>
	<string>{ICON_FILE}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>LSMinimumSystemVersion</key>
	<string>{MIN_MACOS}</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>LSApplicationCategoryType</key>
	<string>{CATEGORY}</string>
</dict>
</plist>
"#
    )
}

/// INSTALL.txt shipped inside the distribution zip (Gatekeeper guidance).
fn install_txt() -> String {
    format!(
        "Vega — macOS (Apple Silicon)\n\
         ============================\n\n\
         系统要求：macOS 11.0+，Apple Silicon（arm64）。\n\n\
         安装：\n\
         1. 解压本 zip，将 Vega.app 拖入「应用程序」（/Applications）。\n\
         2. 首次启动遇 Gatekeeper 拦截（「无法验证开发者」）任选其一：\n\
         a. 在 /Applications 中右键点 Vega.app →「打开」→ 再点「打开」；\n\
         b. 或在终端执行：xattr -cr /Applications/Vega.app\n\n\
         本包为 ad-hoc 签名（未经 Apple 公证），因此其他 Mac 首次启动需要\n\
         上述放行步骤；之后可正常双击启动。\n\n\
         数据位置：\n\
         - 配置：~/.config/vega/config.toml\n\
         - 数据：~/Library/Application Support/ai.vega（bundle id ai.vega，\n\
         与 Keychain 服务同名；版本 {VERSION}）\n\n\
         未公证说明：正式分发请走 Developer ID 签名 + 公证（见\n\
         docs/vega-packaging.md 的 HUMAN PENDING 模板）。\n"
    )
}

/// Runs a macOS built-in tool, inheriting stdio, and fails on nonzero exit.
fn run_tool<A: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[A],
    cwd: Option<&Path>,
) -> Result<()> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

/// Recursively prints the bundle layout (with file sizes) as evidence.
fn print_tree(root: &Path) -> Result<()> {
    println!("{}/", root.display());
    walk_tree(root, "")?;
    Ok(())
}

fn walk_tree(dir: &Path, prefix: &str) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to list {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    let last = entries.len().saturating_sub(1);
    for (index, entry) in entries.iter().enumerate() {
        let branch = if index == last {
            "└── "
        } else {
            "├── "
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            println!("{prefix}{branch}{name}/");
            walk_tree(&path, &format!("{prefix}    "))?;
        } else {
            let size = entry
                .metadata()
                .with_context(|| format!("failed to stat {}", path.display()))?
                .len();
            println!("{prefix}{branch}{name} ({} KB)", size / 1024);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BUNDLE_ID, CATEGORY, EXECUTABLE_NAME, ICONSET_ENTRIES, MIN_MACOS, info_plist};

    #[test]
    fn info_plist_contains_all_required_keys() {
        let plist = info_plist();
        let required = [
            ("CFBundleName", "Vega"),
            ("CFBundleDisplayName", "Vega"),
            // Data compatibility: must equal the paths.rs bundle-id namespace.
            ("CFBundleIdentifier", BUNDLE_ID),
            ("CFBundleExecutable", EXECUTABLE_NAME),
            ("CFBundleIconFile", "Vega"),
            ("CFBundlePackageType", "APPL"),
            ("CFBundleVersion", env!("CARGO_PKG_VERSION")),
            ("CFBundleShortVersionString", env!("CARGO_PKG_VERSION")),
            ("LSMinimumSystemVersion", MIN_MACOS),
            ("LSApplicationCategoryType", CATEGORY),
        ];
        for (key, value) in required {
            assert!(
                plist.contains(&format!("<key>{key}</key>")),
                "Info.plist is missing required key {key}"
            );
            assert!(
                plist.contains(&format!("<string>{value}</string>")),
                "Info.plist is missing required value {value} (for {key})"
            );
        }
        assert!(
            plist.contains("<true/>"),
            "NSHighResolutionCapable must be true"
        );
    }

    #[test]
    fn iconset_covers_the_apple_app_icon_scale_set() {
        // 10 files: 16/32/128/256/512 at @1x plus @2x (topping out at 1024).
        assert_eq!(ICONSET_ENTRIES.len(), 10);
        assert_eq!(ICONSET_ENTRIES[0], (16, "icon_16x16.png"));
        assert_eq!(ICONSET_ENTRIES[9], (1024, "icon_512x512@2x.png"));
        for (size, name) in ICONSET_ENTRIES {
            assert!(name.starts_with("icon_"), "unexpected iconset name {name}");
            assert!(
                *size <= 1024,
                "iconset size {size} exceeds the source canvas"
            );
        }
    }
}
