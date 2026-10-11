use super::ComputerUseError;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ManagedComponents {
    pub cua: PathBuf,
    pub node: PathBuf,
    pub playwright: PathBuf,
}

impl ManagedComponents {
    pub fn detect() -> Result<Self, ComputerUseError> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err(ComputerUseError::UnsupportedPlatform);
        }
        #[cfg(target_os = "macos")]
        {
            let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
                .map_err(|_| ComputerUseError::UnsupportedPlatform)?;
            let version = plist
                .split("<key>ProductVersion</key>")
                .nth(1)
                .and_then(|tail| tail.split("<string>").nth(1))
                .and_then(|tail| tail.split('.').next())
                .and_then(|major| major.parse::<u32>().ok());
            if version.is_none_or(|major| major < 14) {
                return Err(ComputerUseError::UnsupportedPlatform);
            }
        }
        let root = match std::env::var_os("VEGA_COMPUTER_USE_ROOT") {
            Some(root) => PathBuf::from(root),
            None => {
                let executable =
                    std::env::current_exe().map_err(|_| ComputerUseError::MissingComponents)?;
                let root = executable
                    .parent()
                    .and_then(Path::parent)
                    .filter(|root| root.file_name().is_some_and(|name| name == "Contents"))
                    .ok_or(ComputerUseError::MissingComponents)?;
                root.to_owned()
            }
        };
        Self::from_root(&root)
    }

    pub fn from_root(root: &Path) -> Result<Self, ComputerUseError> {
        if !root.is_absolute() {
            return Err(ComputerUseError::IncompatibleComponents);
        }
        use std::io::Read;
        let path = root.join("Resources/computer-use/manifest.json");
        if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(ComputerUseError::MissingComponents);
        }
        let file = std::fs::File::open(path).map_err(|_| ComputerUseError::MissingComponents)?;
        let mut manifest = Vec::new();
        file.take(16 * 1024 + 1)
            .read_to_end(&mut manifest)
            .map_err(|_| ComputerUseError::MissingComponents)?;
        if manifest.len() > 16 * 1024 {
            return Err(ComputerUseError::IncompatibleComponents);
        }
        let value: Value = serde_json::from_slice(&manifest)
            .map_err(|_| ComputerUseError::IncompatibleComponents)?;
        if value["schema_version"] != 1
            || value["platform"] != "darwin-arm64"
            || value["minimum_macos"] != "14.0"
        {
            return Err(ComputerUseError::IncompatibleComponents);
        }
        let mut paths = Vec::new();
        for (key, version, path) in [
            ("cua", "0.34.0", "Helpers/cua-driver"),
            ("node", "24.21.0", "Helpers/node"),
            (
                "playwright_mcp",
                "0.0.83",
                "Resources/computer-use/playwright/cli.js",
            ),
        ] {
            let entry = &value["executors"][key];
            if entry["version"] != version
                || entry["entrypoint"] != path
                || Path::new(path)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(ComputerUseError::IncompatibleComponents);
            }
            let resolved = root.join(path);
            if !resolved.is_file() {
                return Err(ComputerUseError::MissingComponents);
            }
            paths.push(resolved);
        }
        Ok(Self {
            cua: paths.remove(0),
            node: paths.remove(0),
            playwright: paths.remove(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue75_component_manifest_rejects_missing_version_path_and_oversize() {
        let root = tempfile::tempdir().unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_err());
        let manifest_path = root.path().join("Resources/computer-use/manifest.json");
        std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        let mut manifest = serde_json::json!({"schema_version":1,"platform":"darwin-arm64","minimum_macos":"14.0","executors":{
            "cua":{"version":"0.34.0","entrypoint":"Helpers/cua-driver"},
            "node":{"version":"24.21.0","entrypoint":"Helpers/node"},
            "playwright_mcp":{"version":"0.0.83","entrypoint":"Resources/computer-use/playwright/cli.js"}
        }});
        for entry in manifest["executors"].as_object().unwrap().values() {
            let path = root.path().join(entry["entrypoint"].as_str().unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture").unwrap();
        }
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_ok());
        manifest["executors"]["cua"]["version"] = serde_json::json!("0.33.0");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_err());
        manifest["executors"]["cua"]["version"] = serde_json::json!("0.34.0");
        manifest["executors"]["cua"]["entrypoint"] = serde_json::json!("../cua-driver");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_err());
        std::fs::write(&manifest_path, vec![b' '; 16 * 1024 + 1]).unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_err());
        std::fs::remove_file(&manifest_path).unwrap();
        std::fs::create_dir(&manifest_path).unwrap();
        assert!(ManagedComponents::from_root(root.path()).is_err());
    }
}
