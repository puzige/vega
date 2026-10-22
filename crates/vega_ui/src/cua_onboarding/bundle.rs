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
