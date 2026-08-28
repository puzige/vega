//! Credential storage backed by the system Keychain (macOS Keychain via
//! the `keyring` crate; Windows Credential Manager / Secret Service on
//! other platforms).
//!
//! The Keychain service is the constant `"ai.vega"` and the ref_name is
//! used as the account, so a provider named `deepseek` is stored as
//! service `"ai.vega"` / account `"deepseek"`. This keeps a single service
//! constant while the ref_name carries the provider namespace, matching
//! tech-spec §6 (`service=ai.vega.{provider}`).
//!
//! Credential values only ever live inside the Keychain: they are never
//! written to config files, logs, or error messages.

use keyring::Entry;

/// Keychain service name shared by all Vega credentials.
const KEYCHAIN_SERVICE: &str = "ai.vega";

/// [`Entry`] for a credential reference name.
fn entry_for(ref_name: &str) -> Result<Entry, keyring::Error> {
    Entry::new(KEYCHAIN_SERVICE, ref_name)
}

/// Store the credential value for `ref_name` in the Keychain.
///
/// Setting again overwrites the previously stored value.
pub fn set_key(ref_name: &str, key: &str) -> Result<(), keyring::Error> {
    entry_for(ref_name)?.set_password(key)
}

/// Read the credential value for `ref_name` from the Keychain.
///
/// Returns [`keyring::Error::NoEntry`] if nothing is stored yet.
pub fn get_key(ref_name: &str) -> Result<String, keyring::Error> {
    entry_for(ref_name)?.get_password()
}

/// Delete the credential stored under `ref_name`.
///
/// Returns [`keyring::Error::NoEntry`] if nothing is stored.
pub fn delete_key(ref_name: &str) -> Result<(), keyring::Error> {
    entry_for(ref_name)?.delete_credential()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring_core::mock;

    /// Point the keyring default store at the in-memory mock instead of
    /// the real Keychain.
    ///
    /// keyring 4.x initializes its platform store lazily on the first
    /// `Entry` call, which would overwrite a mock installed beforehand;
    /// `store_status()` forces that one-time initialization first, then we
    /// swap in the mock. All tests of this crate run in parallel threads
    /// in one process, so the swap must happen exactly once; afterwards
    /// every entry created through the keystore functions is served by
    /// the mock.
    fn install_mock_store() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = Entry::store_status();
            keyring_core::set_default_store(
                mock::Store::new().expect("mock credential store builds"),
            );
        });
    }

    #[test]
    fn mock_roundtrip_set_get_delete() {
        install_mock_store();
        let ref_name = "vega-t06-mock-roundtrip";
        set_key(ref_name, "test-credential-value").unwrap();
        assert_eq!(get_key(ref_name).unwrap(), "test-credential-value");
        // Setting again replaces the stored value.
        set_key(ref_name, "test-credential-value-2").unwrap();
        assert_eq!(get_key(ref_name).unwrap(), "test-credential-value-2");
        delete_key(ref_name).unwrap();
        let err = get_key(ref_name).unwrap_err();
        assert!(matches!(err, keyring::Error::NoEntry));
    }

    #[test]
    fn mock_get_and_delete_missing_entry_error() {
        install_mock_store();
        let ref_name = "vega-t06-mock-missing";
        assert!(matches!(
            get_key(ref_name).unwrap_err(),
            keyring::Error::NoEntry
        ));
        assert!(matches!(
            delete_key(ref_name).unwrap_err(),
            keyring::Error::NoEntry
        ));
    }

    /// Real-Keychain sanity check, ignored by default: run manually with
    /// `cargo test -p vega_store -- --ignored` (macOS may prompt for
    /// Keychain access). Run alone it never sees the mock, because the
    /// mock is only installed by the tests above.
    #[test]
    #[ignore = "touches the real macOS Keychain; run manually with --ignored"]
    fn real_keychain_roundtrip() {
        let ref_name = "vega-selftest";
        set_key(ref_name, "vega-selftest-value").unwrap();
        assert_eq!(get_key(ref_name).unwrap(), "vega-selftest-value");
        delete_key(ref_name).unwrap();
        assert!(matches!(
            get_key(ref_name).unwrap_err(),
            keyring::Error::NoEntry
        ));
    }
}
