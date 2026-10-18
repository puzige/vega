use super::*;

use vega_conversation::types::{ComputerUseControl, ControlPhase, ConversationStopReason};

fn finish_run(
    stream: &Entity<ConversationStream>,
    control: &ComputerUseControl,
    id: &str,
    cx: &mut TestAppContext,
) {
    control.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(control);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: id.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(100),
            },
            cx,
        );
        stream.finish_composer_run(false, cx);
    });
}
