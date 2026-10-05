use std::cell::RefCell;
use std::fs::File;
use std::os::unix::fs::MetadataExt;

type BodyObserver = Box<dyn Fn(u64, u64)>;

thread_local! {
    static BODY_OBSERVER: RefCell<Option<BodyObserver>> = RefCell::new(None);
    static ASSET_RECHECK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

struct ProbeGuard(bool);

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        if self.0 {
            BODY_OBSERVER.with(|slot| slot.borrow_mut().take());
        } else {
            ASSET_RECHECK.with(|slot| slot.borrow_mut().take());
        }
    }
}

pub(super) fn observe_body_reads<R>(
    observer: impl Fn(u64, u64) + 'static,
    operation: impl FnOnce() -> R,
) -> R {
    BODY_OBSERVER.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(observer));
    });
    let _guard = ProbeGuard(true);
    operation()
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

pub(super) fn before_asset_recheck<R>(
    mutate_owned_fixture: impl FnOnce() + 'static,
    operation: impl FnOnce() -> R,
) -> R {
    ASSET_RECHECK.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(mutate_owned_fixture));
    });
    let _guard = ProbeGuard(false);
    operation()
}

pub(super) fn run_asset_recheck() {
    let mutate_owned_fixture = ASSET_RECHECK.with(|slot| slot.borrow_mut().take());
    if let Some(mutate_owned_fixture) = mutate_owned_fixture {
        mutate_owned_fixture();
    }
}
