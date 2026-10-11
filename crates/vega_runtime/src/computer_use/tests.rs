use super::*;

#[test]
fn issue75_takeover_waits_for_cleanup_and_keeps_single_lease() {
    let a = ComputerUseControl::new();
    let b = ComputerUseControl::new();
    let epoch = a.begin(Capability::Desktop).unwrap();
    let target = DesktopTarget {
        selection_id: "choice".into(),
        application: "Fixture".into(),
        bundle_id: "test.fixture".into(),
        pid: 4,
        window_id: 9,
        title: "Owned".into(),
    };
    a.offer_windows(epoch, vec![target]).unwrap();
    a.approve_window(epoch, "choice").unwrap();
    a.pause();
    assert_eq!(a.resume(a.status().epoch), Err(ComputerUseError::NotReady));
    assert_eq!(b.begin(Capability::Browser), Err(ComputerUseError::Busy));
    assert_eq!(
        a.approve_window(epoch, "choice"),
        Err(ComputerUseError::NotReady)
    );
    a.finish_cleanup();
    a.resume(a.status().epoch).unwrap();
    assert!(a.check_epoch(epoch).is_err());
    assert_eq!(
        a.approve_window(epoch, "choice"),
        Err(ComputerUseError::NotReady)
    );
    a.stop();
    assert_eq!(b.begin(Capability::Browser), Err(ComputerUseError::Busy));
    a.finish_cleanup();
    assert!(b.begin(Capability::Browser).is_ok());
    b.stop();
    b.finish_cleanup();
}

#[test]
fn issue75_audit_contains_no_typed_text_or_target() {
    let input = r#"{"action":"type_text","text":"owned-secret","target":"private-reference"}"#;
    let projected = ComputerUseAudit::project(input, "type_text");
    assert!(ComputerUseAudit::valid(&projected));
    assert!(!projected.contains("owned-secret"));
    assert!(!projected.contains("private-reference"));
}

pub(crate) fn fixture_observation(
    control: &ComputerUseControl,
    epoch: u64,
    index: usize,
) -> Observation {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Observation {
        id: format!("frame-{index}"),
        run_id: control.run_id().into(),
        call_id: format!("call-{index}"),
        epoch,
        capability: Capability::Desktop,
        source: ToolObservationSource::CuaWindow,
        model_image: true,
        target: Target::Desktop(DesktopTarget {
            selection_id: "fixture".into(),
            application: "Fixture".into(),
            bundle_id: "fixture".into(),
            pid: 1,
            window_id: 2,
            title: "private-title".into(),
        }),
        image: Some(crate::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()),
        original_width: 4,
        original_height: 3,
        text: "owned-private-AX".into(),
        snapshot_id: Some(format!("snapshot-{index}")),
        capture_id: Some(format!("capture-{index}")),

        elements: serde_json::json!([]),
    }
}

#[test]
fn issue75_observation_cache_trims_all_old_payloads_and_rejects_late_frames() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let first = fixture_observation(&run.control, epoch, 0);
    run.control
        .connected(epoch, Capability::Desktop, first.target.clone())
        .unwrap();
    let mut messages = Vec::new();
    for index in 0..20 {
        let observation = fixture_observation(&run.control, epoch, index);
        let mut message = crate::ChatMessage::tool_result(&observation.call_id, "durable summary");
        run.control.publish(observation.clone()).unwrap();
        run.apply_observation(&observation.call_id, &mut message);
        messages.push(message);
        run.trim_messages(&mut messages);
        assert_eq!(
            messages
                .iter()
                .filter(|message| !message.images.is_empty())
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.content.contains("owned-private-AX"))
                .count(),
            1
        );
        assert!(run.control.snapshot().observations.len() <= 2);
    }
    assert!(
        run.control
            .observation(Capability::Desktop, "frame-0")
            .is_err()
    );
    run.invalidate_observations();
    run.trim_messages(&mut messages);
    assert!(
        messages
            .iter()
            .all(|message| message.images.is_empty()
                && !message.content.contains("owned-private-AX"))
    );
    run.control.pause();
    assert!(
        run.control
            .publish(fixture_observation(&run.control, epoch, 99))
            .is_err()
    );
    run.control.finish_cleanup();
    run.control.stop();
    run.control.finish_cleanup();
}

#[test]
fn issue75_coordinate_projection_tracks_image_resize_and_rejects_out_of_bounds() {
    let control = ComputerUseControl::new();
    let mut observation = fixture_observation(&control, 1, 0);
    observation.original_width = 8;
    observation.original_height = 6;
    assert_eq!(observation.coordinates(2.0, 1.5).unwrap(), (4.0, 3.0));
    assert!(observation.coordinates(4.0, 0.0).is_err());
    assert!(observation.coordinates(f64::NAN, 0.0).is_err());
    assert!(!format!("{observation:?}").contains("owned-private-AX"));
    assert!(!format!("{:?}", observation.target).contains("private-title"));
}
