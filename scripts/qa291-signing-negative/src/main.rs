use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};

#[path = "../../../xtask/src/sign_update.rs"]
mod sign_update;

static OWNED_ROOT: OnceLock<PathBuf> = OnceLock::new();
static ROOT_CALLS: AtomicUsize = AtomicUsize::new(0);

fn workspace_root() -> Result<PathBuf> {
    ROOT_CALLS.fetch_add(1, Ordering::SeqCst);
    OWNED_ROOT
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("owned root is not initialized"))
}

fn execute() -> Result<bool> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [case, root] = args.as_slice() else {
        bail!("expected case and owned output directory");
    };
    if !matches!(case.as_str(), "missing" | "wrong-public") {
        bail!("unknown negative case");
    }
    let root = PathBuf::from(root);
    if !root.is_absolute() || !root.is_dir() || std::fs::read_dir(&root)?.next().is_some() {
        bail!("owned output directory must be empty");
    }
    OWNED_ROOT
        .set(root.clone())
        .map_err(|_| anyhow::anyhow!("owned root was already initialized"))?;
    let mut public_test_der = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
        0x04, 0x20,
    ];
    public_test_der.extend_from_slice(&[0; 32]);
    if case == "wrong-public" {
        // SAFETY: this standalone driver has one thread and no runtime; only public fixture bytes enter the environment.
        unsafe { std::env::set_var("VEGA_UPDATE_PRIVATE_KEY", STANDARD.encode(public_test_der)) };
    }
    let result = sign_update::run(&["--version".into(), "0.1.63".into()]);
    let error = result.as_ref().err().map(ToString::to_string);
    let outcome = serde_json::json!({
        "case": case,
        "error": error,
        "workspace_root_calls": ROOT_CALLS.load(Ordering::SeqCst),
        "owned_entries": std::fs::read_dir(root)?.count(),
    });
    println!("{outcome}");
    if let Some(error) = error {
        eprintln!("Error: {error}");
    }
    Ok(result.is_err())
}

fn main() -> ExitCode {
    // SAFETY: removal happens before any environment read in this single-thread standalone driver.
    unsafe { std::env::remove_var("VEGA_UPDATE_PRIVATE_KEY") };
    match execute() {
        Ok(true) => ExitCode::FAILURE,
        Ok(false) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("driver setup failed: {error}");
            ExitCode::from(2)
        }
    }
}
