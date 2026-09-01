use super::*;

// S8-T44: the P4 anchor semantics (贴底跟随 / 上翻 detach / 回底 resume) are
// delegated to the pinned GPUI variable-height list's native Tail follow.
// The pure state machine is gone; these tests pin the delegation constants
// and the pure hydration gate that still lives on the model.

#[test]
fn bottom_epsilon_matches_the_native_follow_resume_tolerance() {
    // The list re-engages tail-follow within 1px of the bottom; our top-edge
    // hydration gate mirrors the same epsilon.
    assert_eq!(ANCHOR_EPSILON_PX, 1.0);
}

#[test]
fn compact_subrow_height_is_preserved_for_cards_only() {
    // C4 rule 1: 24px survives only as the compact-subrow rule (diff lines,
    // card-internal rows). It must never apply to the top-level variable
    // height list (no top-level consumer besides card subrows).
    assert_eq!(ROW_HEIGHT, 24.0);
}

#[test]
fn hydration_request_gates_on_top_loading_pause_and_exhaustion() {
    // 顶部 + 有更早历史：请求该 cursor。
    assert_eq!(
        hydration_request(hydration_state(Some(101), false, false), true),
        Some(101)
    );
    // 未到顶部：不请求（用户正在阅读中间内容）。
    assert_eq!(
        hydration_request(hydration_state(Some(101), false, false), false),
        None
    );
    // 一页在飞：不重复请求。
    assert_eq!(
        hydration_request(hydration_state(Some(101), true, false), true),
        None
    );
    // 失败暂停：不再自动请求。
    assert_eq!(
        hydration_request(hydration_state(Some(101), false, true), true),
        None
    );
    // 历史耗尽（含整页末尾的证明性空读之后）：不再请求。
    assert_eq!(
        hydration_request(hydration_state(None, false, false), true),
        None
    );
}
