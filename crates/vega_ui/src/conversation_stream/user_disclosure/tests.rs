use super::*;
use ::core::prelude::v1::test;

fn collapsible(copy: &MessageCopy) {
    let mut state = copy.user_disclosure.borrow_mut();
    state.generation = copy.content_generation();
    state.preview = Some(Rc::new(UserPreview {
        model: StreamModel::from_visible_lines(Vec::new()),
        collapsible: true,
        height: 120.,
        full_height: 480.,
    }));
}

#[test]
fn disclosure_toggle_preserves_text_and_revokes_selection_and_old_intent() {
    let copy = MessageCopy::new("first\n😀中 **styled**");
    collapsible(&copy);
    let original = copy.original_text();
    let generation = copy.content_generation();
    let selection = copy.selection_generation();
    let queued = DisclosureStamp::capture(&copy);
    assert!(queued.matches(&copy.clone()));
    assert!(queued.toggle(&copy));
    assert!(copy.user_disclosure.borrow().expanded);
    assert_ne!(copy.selection_generation(), selection);
    assert_eq!(copy.original_text(), original);
    assert_eq!(copy.content_generation(), generation);
    assert!(!queued.toggle(&copy));
    let expanded = DisclosureStamp::capture(&copy);
    assert!(expanded.toggle(&copy));
    assert!(!copy.user_disclosure.borrow().expanded);
    assert!(!queued.matches(&copy));
    assert!(!expanded.matches(&copy));
}

#[test]
fn disclosure_rejects_equal_text_replacement_and_content_aba() {
    let copy = MessageCopy::new("first");
    let replacement = MessageCopy::new("first");
    collapsible(&copy);
    collapsible(&replacement);
    let queued = DisclosureStamp::capture(&copy);
    assert!(!queued.matches(&replacement));
    assert!(!queued.toggle(&replacement));
    copy.replace_text("second");
    copy.replace_text("first");
    collapsible(&copy);
    assert_eq!(copy.original_text(), "first");
    assert!(!queued.matches(&copy));
    assert!(!queued.toggle(&copy));
    assert!(!copy.user_disclosure.borrow().expanded);
}

#[test]
fn disclosure_rejects_missing_short_and_recomputed_preview() {
    let copy = MessageCopy::new("short");
    assert!(!DisclosureStamp::capture(&copy).toggle(&copy));
    collapsible(&copy);
    let queued = DisclosureStamp::capture(&copy);
    {
        let mut state = copy.user_disclosure.borrow_mut();
        state.preview = Some(Rc::new(UserPreview {
            model: StreamModel::from_visible_lines(Vec::new()),
            collapsible: false,
            height: 24.,
            full_height: 24.,
        }));
        state.revision += 1;
    }
    assert!(!queued.toggle(&copy));
    assert!(!DisclosureStamp::capture(&copy).toggle(&copy));
    assert!(!copy.user_disclosure.borrow().expanded);
}
