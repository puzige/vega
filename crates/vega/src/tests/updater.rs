use super::*;

struct InstallingReset;

impl Drop for InstallingReset {
    fn drop(&mut self) {
        crate::updater::set_installing(false);
    }
}
