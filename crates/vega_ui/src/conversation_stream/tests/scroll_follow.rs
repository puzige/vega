use super::*;

#[test]
fn following_at_bottom_sticks_on_new_content() {
    assert_eq!(
        step(State::Following, 0.0, 600.0, true),
        (State::Following, Action::StickToBottom)
    );
}

#[test]
fn following_at_bottom_without_content_stays() {
    assert_eq!(
        step(State::Following, 0.0, 600.0, false),
        (State::Following, Action::StayPut)
    );
}

#[test]
fn following_within_one_screen_still_jumps_on_content() {
    // 上翻半屏：仍贴底跟随（超过 1 屏才解除跟随）。
    assert_eq!(
        step(State::Following, 300.0, 600.0, true),
        (State::Following, Action::StickToBottom)
    );
}

#[test]
fn following_beyond_one_screen_detaches_and_stays() {
    assert_eq!(
        step(State::Following, 700.0, 600.0, true),
        (State::Detached, Action::StayPut)
    );
    assert_eq!(
        step(State::Following, 700.0, 600.0, false),
        (State::Detached, Action::StayPut)
    );
}

#[test]
fn detach_boundary_is_strictly_more_than_one_viewport() {
    assert_eq!(
        step(State::Following, 600.0, 600.0, true),
        (State::Following, Action::StickToBottom)
    );
    assert_eq!(
        step(State::Following, 600.5, 600.0, true),
        (State::Detached, Action::StayPut)
    );
}

#[test]
fn detached_view_never_jumps_on_new_content() {
    // 脱离后新内容把距离越推越远，仍不跳。
    assert_eq!(
        step(State::Detached, 700.0, 600.0, true),
        (State::Detached, Action::StayPut)
    );
    assert_eq!(
        step(State::Detached, 1200.0, 600.0, true),
        (State::Detached, Action::StayPut)
    );
}

#[test]
fn detached_resumes_when_back_at_bottom() {
    assert_eq!(
        step(State::Detached, 0.0, 600.0, false),
        (State::Following, Action::StickToBottom)
    );
}

#[test]
fn epsilon_counts_as_bottom() {
    assert_eq!(
        step(State::Detached, 0.9, 600.0, false),
        (State::Following, Action::StickToBottom)
    );
    assert_eq!(
        step(State::Following, 1.0, 600.0, true),
        (State::Following, Action::StickToBottom)
    );
}

#[test]
fn zero_viewport_disables_detach_rule() {
    // 首帧布局前 viewport=0：只跟随，不误判脱离。
    assert_eq!(
        step(State::Following, 500.0, 0.0, true),
        (State::Following, Action::StickToBottom)
    );
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
