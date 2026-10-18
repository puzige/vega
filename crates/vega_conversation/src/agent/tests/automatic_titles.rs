use super::*;
use crate::types::AutomaticTitleRequest;

#[tokio::test]
async fn automatic_title_controller_rolls_back_claim_with_rejected_assistant_insert() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    store.conn().execute_batch("CREATE TRIGGER fail_assistant BEFORE INSERT ON messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT,'owned failure'); END;").unwrap();
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let (sender, receiver) = std::sync::mpsc::channel();
    let config = PersistenceActorConfig {
        automatic_title: Some(AutomaticTitleRequest::new(
            "original",
            provider.clone(),
            CancellationToken::new(),
            sender,
        )),
        ..Default::default()
    };
    let result = run_thread_task_with_images_and_reasoning(
        &store,
        provider.as_ref(),
        &tools,
        "thread-1",
        "expanded private",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
        None,
        Vec::new(),
    )
    .await;
    assert!(result.is_err());
    assert!(provider.requests().is_empty());
    assert!(receiver.try_recv().is_err());
    let state: (String, String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT title,auto_title_state,auto_title_claim FROM threads WHERE id='thread-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, ("".into(), "eligible".into(), None));
    assert!(
        vega_store::messages::recent(store.conn(), "thread-1", 10)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn automatic_title_image_only_fallback_has_no_image_request_and_empty_draft_no_claim() {
    for with_image in [false, true] {
        let (store, dir, _) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let image = image::RgbImage::from_pixel(1, 1, image::Rgb([20, 40, 60]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let images = if with_image {
            vec![crate::types::ImageAttachment::from_bytes(png.into_inner()).unwrap()]
        } else {
            Vec::new()
        };
        let title_provider = Arc::new(MockProvider::new(vec![ScriptStep::delay(
            Duration::from_secs(30),
        )]));
        let primary = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let (sender, receiver) = std::sync::mpsc::channel();
        let title_cancel = CancellationToken::new();
        let config = PersistenceActorConfig {
            automatic_title: Some(AutomaticTitleRequest::new(
                "",
                title_provider.clone(),
                title_cancel.clone(),
                sender,
            )),
            ..Default::default()
        };
        run_thread_task_with_images_and_reasoning(
            &store,
            &primary,
            &tools,
            "thread-1",
            "",
            "",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            config,
            None,
            None,
            None,
            images,
        )
        .await
        .unwrap();
        if with_image {
            receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(
                vega_store::threads::find(store.conn(), "thread-1")
                    .unwrap()
                    .unwrap()
                    .title,
                "图片对话"
            );
            for _ in 0..100 {
                if !title_provider.requests().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let requests = title_provider.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].messages[1].content, "图片对话");
            assert!(
                requests[0]
                    .messages
                    .iter()
                    .all(|message| message.images.is_empty())
            );
        } else {
            assert!(receiver.try_recv().is_err());
            assert!(title_provider.requests().is_empty());
            assert_eq!(
                vega_store::threads::find(store.conn(), "thread-1")
                    .unwrap()
                    .unwrap()
                    .title,
                ""
            );
        }
        title_cancel.cancel();
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_err());
    }
}
