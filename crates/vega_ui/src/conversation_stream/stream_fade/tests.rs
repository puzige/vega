use super::*;
use core::prelude::v1::test;
use gpui_kit::{StrikethroughStyle, UnderlineStyle, hsla, px};

#[test]
fn native_ranges_require_exact_part_text_and_preserve_multibyte_append_boundaries() {
    let now = Instant::now();
    let mut fade = StreamFade::default();
    fade.register((9, 0, 0, TextPart::Body), "A😀".into(), now);
    fade.register((9, 0, 0, TextPart::Body), "A😀中".into(), now);
    fade.register((9, 0, 0, TextPart::Marker), "1. ".into(), now);
    fade.register((9, 0, 1, TextPart::Cell(2)), "A😀中".into(), now);
    let frame = fade.frame_at(now + Duration::from_millis(80), false);
    let ranges = frame.ranges(9, 0, 0, TextPart::Body, "A😀中").unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].range, 0..5);
    assert_eq!(ranges[1].range, 5..8);
    for range in ranges {
        assert!("A😀中".get(range.range.clone()).is_some());
        assert!(range.opacity > 0. && range.opacity < 1.);
    }
    assert!(frame.ranges(9, 0, 0, TextPart::Body, "A😀").is_none());
    assert!(frame.ranges(10, 0, 0, TextPart::Body, "A😀中").is_none());
    assert!(frame.ranges(9, 1, 0, TextPart::Body, "A😀中").is_none());
    assert!(frame.ranges(9, 0, 0, TextPart::Cell(2), "A😀中").is_none());
    assert_eq!(
        frame.ranges(9, 0, 0, TextPart::Marker, "1. ").unwrap()[0].range,
        0..3
    );
    assert_eq!(
        frame.ranges(9, 0, 1, TextPart::Cell(2), "A😀中").unwrap()[0].range,
        0..8
    );
}

#[test]
fn retained_native_frame_cannot_adopt_newer_text_and_replacement_clears_old_fades() {
    let now = Instant::now();
    let mut fade = StreamFade::default();
    fade.register((4, 0, 0, TextPart::Body), "old".into(), now);
    let frame = fade.frame_at(now + Duration::from_millis(100), false);
    fade.register(
        (4, 0, 0, TextPart::Body),
        "old plus".into(),
        now + Duration::from_millis(101),
    );
    assert!(frame.ranges(4, 0, 0, TextPart::Body, "old").is_some());
    assert!(frame.ranges(4, 0, 0, TextPart::Body, "old plus").is_none());
    let appended = fade.frame_at(now + Duration::from_millis(110), false);
    assert_eq!(
        appended
            .ranges(4, 0, 0, TextPart::Body, "old plus")
            .unwrap()
            .len(),
        2
    );
    fade.register(
        (4, 0, 0, TextPart::Body),
        "replacement".into(),
        now + Duration::from_millis(120),
    );
    assert!(
        fade.frame_at(now + Duration::from_millis(130), false)
            .parts
            .is_empty()
    );
    assert!(
        frame
            .ranges(4, 0, 0, TextPart::Body, "replacement")
            .is_none()
    );
}

#[test]
fn native_range_alpha_and_legacy_foreground_background_decorations_share_one_frame() {
    let now = Instant::now();
    let mut fade = StreamFade::default();
    fade.register((2, 0, 0, TextPart::Body), "old".into(), now);
    fade.frame_at(now + Duration::from_millis(701), false);
    fade.register(
        (2, 0, 0, TextPart::Body),
        "old😀".into(),
        now + Duration::from_millis(702),
    );
    let frame = fade.frame_at(now + Duration::from_millis(900), false);
    let alpha = frame.ranges(2, 0, 0, TextPart::Body, "old😀").unwrap()[0].opacity;
    let original = TextRun {
        len: 7,
        color: hsla(0.3, 0.5, 0.7, 0.8),
        background_color: Some(hsla(0.2, 0.6, 0.7, 0.6)),
        underline: Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(hsla(0.1, 0.2, 0.3, 0.4)),
            wavy: true,
        }),
        strikethrough: Some(StrikethroughStyle {
            thickness: px(2.),
            color: Some(hsla(0.4, 0.3, 0.2, 0.5)),
        }),
        ..Default::default()
    };
    let runs = frame
        .line(2, 0)
        .runs(0, TextPart::Body, vec![original.clone()]);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].len, 3);
    assert_eq!(runs[0].color, original.color);
    assert_eq!(runs[1].len, 4);
    assert_eq!(runs[1].color.a, original.color.a * alpha);
    assert_eq!(
        runs[1].background_color.unwrap().a,
        original.background_color.unwrap().a * alpha
    );
    assert_eq!(
        runs[1].underline.unwrap().color.unwrap().a,
        original.underline.unwrap().color.unwrap().a * alpha
    );
    assert_eq!(
        runs[1].strikethrough.unwrap().color.unwrap().a,
        original.strikethrough.unwrap().color.unwrap().a * alpha
    );
    assert!(runs[1].underline.unwrap().wavy);
    assert_eq!(runs[1].strikethrough.unwrap().thickness, px(2.));
}

#[test]
fn finish_converges_from_current_alpha_and_motion_disable_does_not_rearm() {
    let now = Instant::now();
    let part = (3, 0, 0, TextPart::Body);
    let mut fade = StreamFade::default();
    fade.register(part, "text".into(), now);
    let before = fade
        .frame_at(now + Duration::from_millis(100), false)
        .ranges(3, 0, 0, TextPart::Body, "text")
        .unwrap()[0]
        .opacity;
    fade.finish_at(now + Duration::from_millis(100));
    let start = fade
        .frame_at(now + Duration::from_millis(100), false)
        .ranges(3, 0, 0, TextPart::Body, "text")
        .unwrap()[0]
        .opacity;
    assert_eq!(before, start);
    let progressing = fade
        .frame_at(now + Duration::from_millis(175), false)
        .ranges(3, 0, 0, TextPart::Body, "text")
        .unwrap()[0]
        .opacity;
    assert!(progressing > start && progressing < 1.);
    assert!(
        fade.frame_at(now + Duration::from_millis(250), false)
            .parts
            .is_empty()
    );
    fade.register(part, "text more".into(), now + Duration::from_millis(260));
    assert!(
        !fade
            .frame_at(now + Duration::from_millis(270), false)
            .parts
            .is_empty()
    );
    assert!(
        fade.frame_at(now + Duration::from_millis(271), true)
            .parts
            .is_empty()
    );
    assert!(
        fade.frame_at(now + Duration::from_millis(272), false)
            .parts
            .is_empty()
    );
    fade.register(
        part,
        "text more later".into(),
        now + Duration::from_millis(280),
    );
    assert!(
        fade.frame_at(now + Duration::from_millis(2781), false)
            .parts
            .is_empty()
    );
}

#[test]
fn hiding_drops_pending_animation_without_replaying_retained_text_on_reveal() {
    let now = Instant::now();
    let mut fade = StreamFade::default();
    fade.register((1, 0, 0, TextPart::Body), "old😀".into(), now);
    fade.hide();
    assert!(
        fade.frame_at(now + Duration::from_millis(1), false)
            .parts
            .is_empty()
    );
    fade.register(
        (1, 0, 0, TextPart::Body),
        "old😀".into(),
        now + Duration::from_millis(2),
    );
    assert!(
        fade.frame_at(now + Duration::from_millis(3), false)
            .parts
            .is_empty()
    );
    fade.register(
        (1, 0, 0, TextPart::Body),
        "old😀new".into(),
        now + Duration::from_millis(4),
    );
    let frame = fade.frame_at(now + Duration::from_millis(5), false);
    let ranges = frame.ranges(1, 0, 0, TextPart::Body, "old😀new").unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].range, "old😀".len().."old😀new".len());
    assert!(ranges[0].opacity > 0. && ranges[0].opacity < 1.);
}

#[test]
fn maximum_age_finishes_a_segment_even_when_its_start_is_still_queued() {
    let now = Instant::now();
    let mut fade = StreamFade::default();
    fade.register((1, 0, 0, TextPart::Body), "queued".into(), now);
    for segment in fade.active.values_mut() {
        segment.start = now + Duration::from_secs(5);
    }
    let waiting = fade.frame_at(now + Duration::from_millis(2499), false);
    let ranges = waiting.ranges(1, 0, 0, TextPart::Body, "queued").unwrap();
    assert_eq!(ranges[0].opacity, 0.);
    assert!(
        fade.frame_at(now + Duration::from_millis(2500), false)
            .parts
            .is_empty()
    );
}
