use super::*;
use vega_runtime::computer_use::{ComputerUseControl, test_support};

fn persist(store: &Store, project: &str, event: &RuntimeEvent) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        project,
        "thread-1",
        "capture-owner",
        "mock-model",
        false,
        "Screenshot attached.",
        &mut 2,
        event,
        None,
    )
}
