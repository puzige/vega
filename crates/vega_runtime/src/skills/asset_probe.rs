use std::cell::RefCell;
use std::fs::File;
use std::os::unix::fs::MetadataExt;

type BodyObserver = Box<dyn Fn(u64, u64)>;

thread_local! {
    static BODY_OBSERVER: RefCell<Option<BodyObserver>> = RefCell::new(None);
    static ASSET_RECHECK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn record_body_read(file: &File) {
    BODY_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref()
            && let Ok(metadata) = file.metadata()
        {
            observer(metadata.dev(), metadata.ino());
        }
    });
}

pub(super) fn run_asset_recheck() {
    let mutate_owned_fixture = ASSET_RECHECK.with(|slot| slot.borrow_mut().take());
    if let Some(mutate_owned_fixture) = mutate_owned_fixture {
        mutate_owned_fixture();
    }
}
