use super::*;

pub(in crate::conversation_stream) fn restored(text: &str, cx: &mut App) -> Entity<ThinkingBlock> {
    cx.new(|cx| {
        let mut block = ThinkingBlock::new(cx);
        block.append(text, false, THINKING_VIEW_BYTES);
        block.set_stage_expanded(true, cx);
        block
    })
}
