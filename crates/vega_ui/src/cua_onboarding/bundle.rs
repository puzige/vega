pub fn bundle_path(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    let _ = executable;
    None
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
