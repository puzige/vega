pub fn bundle_path(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    if executable.file_name()? != "vega" {
        return None;
    }
    let macos = executable.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    let name = bundle.file_name()?;
    if !name.to_str()?.ends_with(".app") || !bundle.is_absolute() {
        return None;
    }
    Some(bundle.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn installed_bundle_resolves_to_app_directory() {
        let resolved = bundle_path(Path::new("/Applications/Vega.app/Contents/MacOS/vega"));

        assert_eq!(
            resolved,
            Some(std::path::PathBuf::from("/Applications/Vega.app"))
        );
    }

    #[test]
    fn development_build_has_no_bundle() {
        let resolved = bundle_path(Path::new("/Users/x/Workspace/vega/target/debug/vega"));

        assert_eq!(resolved, None);
    }

    #[test]
    fn other_executable_name_has_no_bundle() {
        let resolved = bundle_path(Path::new("/Applications/Vega.app/Contents/MacOS/other"));

        assert_eq!(resolved, None);
    }

    #[test]
    fn deeper_layout_has_no_bundle() {
        let resolved = bundle_path(Path::new(
            "/Applications/Vega.app/Contents/MacOS/vega/extra",
        ));

        assert_eq!(resolved, None);
    }
}
