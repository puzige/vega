pub(crate) fn format_file_size(size: u64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    let units = ["KB", "MB", "GB", "TB", "PB", "EB"];
    let mut value = size as f64 / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < units.len() {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}
