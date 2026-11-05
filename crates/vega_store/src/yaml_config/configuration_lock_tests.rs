use super::*;

#[test]
fn syscall_lock_errors_retain_original_os_code_and_are_not_retryable_busy() {
    for code in [
        libc::EIO,
        libc::EBADF,
        libc::ENOTSUP,
        libc::EACCES,
        libc::EINTR,
        libc::EAGAIN,
    ] {
        let source = io::Error::from_raw_os_error(code);
        let kind = source.kind();
        let error = lock_error(std::fs::TryLockError::Error(source));
        assert_eq!(error.raw_os_error(), Some(code));
        assert_eq!(error.kind(), kind);
        assert!(lock_busy(&error).is_none());
    }
    let busy = lock_error(std::fs::TryLockError::WouldBlock);
    assert_eq!(lock_busy(&busy), Some(ConfigurationLockBusy::File));
}
