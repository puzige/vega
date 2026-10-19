use std::{future::Future, io::Cursor, pin::Pin, time::Duration};

use futures::FutureExt;
use image::{DynamicImage, ImageFormat, RgbaImage};
use tokio_util::sync::CancellationToken;

use crate::{
    link_preview::classify_target,
    link_preview_test_support::{ManualClock, Reply, ScriptedNetwork},
    types::{LinkPreviewFailure, LinkSiteKind},
};

const PAGE: &str = "https://example.org/article?q=one";

fn html(title: &str) -> Reply {
    Reply::html(format!("<html><head><title>{title}</title></head></html>"))
}

async fn assert_pending<F: Future>(mut future: Pin<&mut F>) {
    for _ in 0..8 {
        assert!(future.as_mut().now_or_never().is_none());
        tokio::task::yield_now().await;
    }
}

fn png(width: u32, height: u32, noise: bool) -> Vec<u8> {
    let mut pixels = RgbaImage::new(width, height);
    let mut value = 0x04c1_28e7_u32;
    for pixel in pixels.pixels_mut() {
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        *pixel = image::Rgba(if noise {
            value.to_le_bytes()
        } else {
            [72, 120, 160, 255]
        });
    }
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

#[test]
fn site_classification_uses_the_actual_hostname_and_preserves_the_open_target() {
    for (url, expected) in [
        (
            "https://github.com/puzige/vega#discussion",
            LinkSiteKind::Github,
        ),
        ("https://GIST.GITHUB.COM/a", LinkSiteKind::Github),
        ("https://github.com.example.org/a", LinkSiteKind::Web),
        ("https://example.org/github.com", LinkSiteKind::Web),
        (
            "mailto:reader@example.org?subject=Hello",
            LinkSiteKind::Mail,
        ),
    ] {
        let target = classify_target(url).unwrap();
        assert_eq!(target.site, expected, "{url}");
        assert_eq!(target.original_target, url);
    }
    assert_eq!(
        classify_target("https://github.com@example.org/").unwrap_err(),
        LinkPreviewFailure::Blocked
    );
}

#[tokio::test]
async fn anonymous_metadata_decoding_uses_the_effective_page_for_relative_images() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(PAGE, Reply::redirect("https://publisher.org/posts/42"));
    network.respond(
        "https://publisher.org/posts/42",
        Reply::html(
            "<html><head><META content='Open &amp; clear' property='OG:TITLE'>\
             <meta property='og:description' content='A &#x4e2d;&#25991; &quot;summary&quot;'>\
             <meta content='../cover.png?size=2&amp;safe=1' property='og:image'>\
             <meta property='og:site_name' content='github.com'>\
             <meta property='og:url' content='https://attacker.org/'>\
             <title>Fallback</title></head><body>Body is not a summary</body></html>",
        ),
    );
    network.respond(
        "https://publisher.org/cover.png?size=2&safe=1",
        Reply::bytes("image/png", png(1024, 512, false)),
    );
    let original = format!("{PAGE}#comment-9");
    let result = network
        .service(&clock)
        .load(&original, false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.original_target, original);
    assert_eq!(result.effective_target, "https://publisher.org/posts/42");
    assert_eq!(result.host, "publisher.org");
    assert_eq!(result.site, LinkSiteKind::Web);
    assert_eq!(result.title.as_deref(), Some("Open & clear"));
    assert_eq!(result.description.as_deref(), Some("A 中文 \"summary\""));
    let image = result.image.unwrap();
    assert_eq!((image.width, image.height), (512, 256));
    assert_eq!(image::guess_format(&image.png).unwrap(), ImageFormat::Png);
    let decoded = image::load_from_memory(&image.png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (512, 256));
    let requests = network.requests();
    assert_eq!(requests.len(), 3);
    for request in requests {
        assert!(!request.pinned_addresses.is_empty());
        assert!(
            request
                .pinned_addresses
                .iter()
                .all(|address| address.ip().to_string() == "93.184.216.34")
        );
        for (name, _) in &request.headers {
            assert!(!matches!(
                name.to_ascii_lowercase().as_str(),
                "cookie" | "authorization" | "proxy-authorization" | "referer"
            ));
        }
        assert!(request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("accept-encoding") && value == "identity"
        }));
    }
}

#[tokio::test]
async fn metadata_parser_respects_rcdata_rawtext_templates_duplicates_and_head_scope() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let samples = [
        (
            "<html><head><script>const s=\"<meta property='og:title' content='Fake script'>\";</script>\
             <style>.x{content:\"<meta property='og:title' content='Fake style'>\"}</style>\
             <template><meta property='og:title' content='Fake template'></template>\
             <!-- <meta property='og:title' content='Fake comment'> -->\
             <meta property='og:title' content='  '>\
             <meta content=' First &amp; real ' property='og:title'>\
             <meta property='og:title' content='Second'>\
             <meta name='description' content='Standard &lt;description&gt;'>\
             <title>Fallback</title></head><body><meta property='og:title' content='Fake body'></body></html>",
            "First & real",
            Some("Standard <description>"),
        ),
        (
            "<html><head><title>RCDATA <b>literal</b> &amp; text</title>\
             <meta name=description content='line&#10;one&#9;two'></head></html>",
            "RCDATA <b>literal</b> & text",
            Some("line one two"),
        ),
        (
            "\u{feff}<html><head><meta property=og:title content='真实标题'>\
             <meta name=description content=Fallback>\
             <meta property=og:description content='OG wins'></head></html>",
            "真实标题",
            Some("OG wins"),
        ),
    ];
    for (index, (source, title, summary)) in samples.into_iter().enumerate() {
        let url = format!("https://example.org/parser/{index}");
        let mut reply = Reply::html(source);
        let body = reply.chunks.pop().unwrap().unwrap();
        reply.chunks = body.chunks(7).map(|bytes| Ok(bytes.to_vec())).collect();
        network.respond(&url, reply);
        let result = network
            .service(&clock)
            .load(&url, false, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.title.as_deref(), Some(title));
        assert_eq!(result.description.as_deref(), summary);
        assert!(result.image.is_none());
    }
}

#[tokio::test]
async fn absent_metadata_stays_absent_and_only_the_first_usable_declared_image_is_loaded() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(
        PAGE,
        Reply::html(
            "<html><head></head><body><h1>Not a title</h1><img src='/body.png'></body></html>",
        ),
    );
    let service = network.service(&clock);
    let result = service
        .load(PAGE, false, CancellationToken::new())
        .await
        .unwrap();
    assert!(result.title.is_none());
    assert!(result.description.is_none());
    assert!(result.image.is_none());
    assert_eq!(network.requests().len(), 1);
    let second = "https://example.org/declared";
    network.respond(second, Reply::html("<head><meta property=og:image content=''><meta property=og:image content='/one.png'><meta property=og:image content='/two.png'></head>"));
    network.respond(
        "https://example.org/one.png",
        Reply::bytes("image/png", png(32, 16, false)),
    );
    let result = service
        .load(second, false, CancellationToken::new())
        .await
        .unwrap();
    let image = result.image.unwrap();
    assert_eq!((image.width, image.height), (32, 16));
    assert_eq!(
        network
            .requests()
            .iter()
            .filter(|request| request.url.ends_with(".png"))
            .map(|request| request.url.as_str())
            .collect::<Vec<_>>(),
        ["https://example.org/one.png"]
    );
}

#[tokio::test]
async fn fields_are_bounded_at_unicode_boundaries_without_fabricating_missing_content() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(PAGE, Reply::html(format!("<head><meta property=og:title content='{}'><meta property=og:description content='{}'></head>", "中".repeat(600), "é".repeat(2100))));
    let result = network
        .service(&clock)
        .load(PAGE, false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.title.unwrap(), "中".repeat(512));
    assert_eq!(result.description.unwrap(), "é".repeat(2048));
    assert!(result.image.is_none());
}

#[tokio::test]
async fn error_pages_non_html_and_unsupported_encoding_do_not_become_successful_previews() {
    for (status, mime, expected) in [
        (401, "text/html", LinkPreviewFailure::RequiresLogin),
        (403, "text/html", LinkPreviewFailure::RequiresLogin),
        (500, "text/html", LinkPreviewFailure::Network),
        (200, "application/pdf", LinkPreviewFailure::Unsupported),
        (200, "image/png", LinkPreviewFailure::Unsupported),
        (
            200,
            "text/html; charset=windows-1252",
            LinkPreviewFailure::Malformed,
        ),
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.respond(
            PAGE,
            Reply::bytes(mime, b"<head><title>Error page</title></head>".to_vec()).status(status),
        );
        assert_eq!(
            network
                .service(&clock)
                .load(PAGE, false, CancellationToken::new())
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(network.requests().len(), 1);
    }
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(
        PAGE,
        Reply::bytes("text/html", vec![0xff, 0xfe, 0xc0, 0xaf]),
    );
    assert_eq!(
        network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap_err(),
        LinkPreviewFailure::Malformed
    );
}

#[tokio::test]
async fn page_length_is_checked_both_before_reading_and_across_streamed_chunks() {
    for declared in [true, false] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        let mut reply = html("Bounded");
        if declared {
            reply = reply.header("content-length", "2097153");
        } else {
            reply.chunks = vec![Ok(vec![b' '; 1_048_576]), Ok(vec![b' '; 1_048_577])];
        }
        network.respond(PAGE, reply);
        assert_eq!(
            network
                .service(&clock)
                .load(PAGE, false, CancellationToken::new())
                .await
                .unwrap_err(),
            LinkPreviewFailure::TooLarge
        );
    }
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let mut body = b"<head><title>At boundary</title></head>".to_vec();
    body.resize(2_097_152, b' ');
    network.respond(PAGE, Reply::bytes("text/html", body));
    let result = network
        .service(&clock)
        .load(PAGE, false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.title.as_deref(), Some("At boundary"));
}

#[tokio::test]
async fn invalid_mime_oversized_corrupt_or_overpixel_images_preserve_real_text() {
    let images = [
        Reply::bytes("image/svg+xml", b"<svg/>".to_vec()),
        Reply::bytes("image/png", b"not a png".to_vec()),
        Reply::bytes("image/jpeg", png(20, 10, false)),
        Reply::bytes("image/png", vec![0; 5_242_881]),
        Reply::bytes("image/png", png(4000, 2001, false)),
        Reply::bytes("image/png", png(20, 10, false)).header("content-length", "5242881"),
        Reply::bytes("text/html", b"<head><title>Login</title></head>".to_vec()).status(403),
    ];
    for image in images {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.respond(PAGE, Reply::html("<head><title>Keep me</title><meta property=og:description content='Real summary'><meta property=og:image content='/cover'></head>"));
        network.respond("https://example.org/cover", image);
        let result = network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.title.as_deref(), Some("Keep me"));
        assert_eq!(result.description.as_deref(), Some("Real summary"));
        assert!(result.image.is_none());
    }
}

#[tokio::test]
async fn public_destination_policy_rejects_literal_variants_and_non_public_dns_answers() {
    for target in [
        "https://localhost/",
        "https://api.localhost/",
        "https://127.0.0.1/",
        "https://127.1/",
        "https://2130706433/",
        "https://0x7f000001/",
        "https://[::1]/",
        "https://[::ffff:127.0.0.1]/",
        "https://10.0.0.1/",
        "https://100.64.0.1/",
        "https://169.254.1.1/",
        "https://172.16.0.1/",
        "https://192.0.0.8/",
        "https://192.0.2.1/",
        "https://192.88.99.1/",
        "https://192.168.1.1/",
        "https://198.18.0.1/",
        "https://198.51.100.1/",
        "https://203.0.113.1/",
        "https://224.0.0.1/",
        "https://240.0.0.1/",
        "https://0.0.0.0/",
        "https://[2001:db8::1]/",
        "https://[2002:0808:0808::1]/",
        "https://[2001::1]/",
        "https://[3ffe::1]/",
        "https://[3fff::1]/",
        "https://[fc00::1]/",
        "https://[fe80::1]/",
        "https://[ff02::1]/",
        "https://github.com@example.org/",
        "https://user:pass@example.org/",
        "file:///tmp/a",
        "javascript:alert(1)",
        "https://example.org/\nsecret",
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        assert!(
            network
                .service(&clock)
                .load(target, false, CancellationToken::new())
                .await
                .is_err(),
            "{target}"
        );
        assert!(
            network.requests().is_empty(),
            "forbidden connection: {target}"
        );
    }
    for addresses in [
        vec!["10.1.2.3"],
        vec!["93.184.216.34", "127.0.0.1"],
        vec!["::ffff:192.168.1.1"],
        vec![],
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.answer("example.org", &addresses);
        assert_eq!(
            network
                .service(&clock)
                .load(PAGE, false, CancellationToken::new())
                .await
                .unwrap_err(),
            LinkPreviewFailure::Blocked
        );
        assert!(network.requests().is_empty());
    }
}

#[tokio::test]
async fn accepted_addresses_are_pinned_once_and_an_unapproved_peer_is_rejected() {
    for peer in ["93.184.216.34:443", "127.0.0.1:443", "8.8.8.8:443"] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.answer("example.org", &["93.184.216.34"]);
        network.answer("example.org", &["127.0.0.1"]);
        let mut reply = html("Pinned");
        reply.peer = peer.parse().unwrap();
        network.respond(PAGE, reply);
        let result = network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await;
        assert_eq!(network.dns_calls(), [("example.org".into(), 443)]);
        assert_eq!(
            network.requests()[0].pinned_addresses,
            ["93.184.216.34:443".parse::<std::net::SocketAddr>().unwrap()]
        );
        if peer == "93.184.216.34:443" {
            assert_eq!(result.unwrap().title.as_deref(), Some("Pinned"));
        } else {
            assert_eq!(result.unwrap_err(), LinkPreviewFailure::Blocked);
        }
    }
}

#[tokio::test]
async fn every_redirect_and_image_hop_rechecks_the_same_public_policy() {
    for location in [
        "http://127.0.0.1/private",
        "https://internal.example/private",
        "file:///secret",
        "https://u:p@example.org/",
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.answer("internal.example", &["192.168.0.2"]);
        network.respond(PAGE, Reply::redirect(location));
        assert!(
            network
                .service(&clock)
                .load(PAGE, false, CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(
            network
                .requests()
                .iter()
                .map(|request| request.url.as_str())
                .collect::<Vec<_>>(),
            [PAGE]
        );
    }
    for image_redirect in [false, true] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        let candidate = if image_redirect {
            "/safe.png"
        } else {
            "http://127.0.0.1/private.png"
        };
        network.respond(PAGE, Reply::html(format!("<head><title>Safe text</title><meta property=og:image content='{candidate}'></head>")));
        if image_redirect {
            network.respond(
                "https://example.org/safe.png",
                Reply::redirect("http://127.0.0.1/private.png"),
            );
        }
        let result = network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.title.as_deref(), Some("Safe text"));
        assert!(result.image.is_none());
        assert_eq!(network.requests().len(), if image_redirect { 2 } else { 1 });
    }
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.answer("example.org", &["93.184.216.34"]);
    network.answer("example.org", &["10.0.0.1"]);
    network.respond(PAGE, Reply::redirect("/next"));
    assert_eq!(
        network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap_err(),
        LinkPreviewFailure::Blocked
    );
    assert_eq!(network.requests().len(), 1);
    assert_eq!(network.dns_calls().len(), 2);
}

#[tokio::test]
async fn redirects_have_a_three_hop_budget_and_require_a_valid_location() {
    for count in [3, 4] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        for index in 0..count {
            network.respond(
                &format!("https://example.org/{index}"),
                Reply::redirect(&format!("/{}", index + 1)),
            );
        }
        network.respond(&format!("https://example.org/{count}"), html("Final"));
        let result = network
            .service(&clock)
            .load("https://example.org/0", false, CancellationToken::new())
            .await;
        assert_eq!(network.requests().len(), 4);
        if count == 3 {
            assert_eq!(result.unwrap().title.as_deref(), Some("Final"));
        } else {
            assert!(result.is_err());
        }
    }
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(PAGE, html("Not valid redirect").status(302));
    assert!(
        network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(network.requests().len(), 1);
}

#[tokio::test]
async fn url_limits_count_utf8_bytes_and_mail_never_reaches_the_network() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let target = format!("https://example.org/{}", "中".repeat(3000));
    assert!(target.len() > 8192);
    assert!(target.chars().count() < 8192);
    assert!(
        network
            .service(&clock)
            .load(&target, false, CancellationToken::new())
            .await
            .is_err()
    );
    let result = network
        .service(&clock)
        .load("mailto:reader@example.org", false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.site, LinkSiteKind::Mail);
    assert!(result.title.is_none() && result.description.is_none() && result.image.is_none());
    assert!(network.requests().is_empty());
    assert!(network.dns_calls().is_empty());
}

#[tokio::test]
async fn dns_and_whole_load_deadlines_cancel_without_caching_a_cancelled_load() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let _gate = network.gate_dns("example.org");
    let service = network.service(&clock);
    let mut pending = Box::pin(service.load(PAGE, false, CancellationToken::new()));
    assert_pending(pending.as_mut()).await;
    clock.advance(Duration::from_millis(1999));
    assert_pending(pending.as_mut()).await;
    clock.advance(Duration::from_millis(1));
    assert_eq!(pending.await.unwrap_err(), LinkPreviewFailure::Timeout);
    assert!(network.requests().is_empty());
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let _gate = network.gated(PAGE, html("Too late"));
    let service = network.service(&clock);
    let mut pending = Box::pin(service.load(PAGE, false, CancellationToken::new()));
    assert_pending(pending.as_mut()).await;
    clock.advance(Duration::from_millis(9999));
    assert_pending(pending.as_mut()).await;
    clock.advance(Duration::from_millis(1));
    assert_eq!(pending.await.unwrap_err(), LinkPreviewFailure::Timeout);
}

#[tokio::test]
async fn positive_and_negative_cache_ttls_query_keys_and_retry_follow_the_contract() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let service = network.service(&clock);
    network.respond(PAGE, html("First"));
    for fragment in ["#one", "#two"] {
        let original = format!("{PAGE}{fragment}");
        let result = service
            .load(&original, false, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.original_target, original);
        assert_eq!(result.title.as_deref(), Some("First"));
    }
    assert_eq!(network.requests().len(), 1);
    clock.advance(Duration::from_secs(1799));
    assert_eq!(
        service
            .load(PAGE, true, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("First")
    );
    assert_eq!(network.requests().len(), 1);
    clock.advance(Duration::from_secs(1));
    network.respond(PAGE, html("Fresh"));
    assert_eq!(
        service
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Fresh")
    );
    let other_query = "https://example.org/article?q=two";
    network.fail(other_query, LinkPreviewFailure::Network);
    assert_eq!(
        service
            .load(other_query, false, CancellationToken::new())
            .await
            .unwrap_err(),
        LinkPreviewFailure::Network
    );
    let before = network.requests().len();
    clock.advance(Duration::from_secs(29));
    assert!(
        service
            .load(other_query, false, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(network.requests().len(), before);
    network.respond(other_query, html("Recovered"));
    assert_eq!(
        service
            .load(other_query, true, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Recovered")
    );
    let fail_url = "https://example.org/expiry";
    network.fail(fail_url, LinkPreviewFailure::Network);
    assert!(
        service
            .load(fail_url, false, CancellationToken::new())
            .await
            .is_err()
    );
    clock.advance(Duration::from_secs(30));
    network.respond(fail_url, html("After TTL"));
    assert_eq!(
        service
            .load(fail_url, false, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("After TTL")
    );
}

#[tokio::test]
async fn cancelling_one_singleflight_waiter_preserves_the_other_and_last_waiter_cancels_work() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let gate = network.gated(PAGE, html("Shared"));
    let service = network.service(&clock);
    let first_token = CancellationToken::new();
    let second_token = CancellationToken::new();
    let first_target = format!("{PAGE}#first");
    let second_target = format!("{PAGE}#second");
    let mut first = Box::pin(service.load(&first_target, false, first_token.clone()));
    let mut second = Box::pin(service.load(&second_target, false, second_token));
    assert_pending(first.as_mut()).await;
    assert_pending(second.as_mut()).await;
    assert_eq!(network.requests().len(), 1);
    first_token.cancel();
    assert_eq!(first.await.unwrap_err(), LinkPreviewFailure::Cancelled);
    assert_eq!(network.cancellations(), 0);
    assert!(!network.request_cancelled(0));
    gate.send(()).unwrap();
    let result = second.await.unwrap();
    assert_eq!(result.original_target, second_target);
    assert_eq!(result.title.as_deref(), Some("Shared"));
    let abandoned = "https://example.org/abandoned";
    let _gate = network.gated(abandoned, html("Discard"));
    let token = CancellationToken::new();
    let mut pending = Box::pin(service.load(abandoned, false, token.clone()));
    assert_pending(pending.as_mut()).await;
    token.cancel();
    assert_eq!(pending.await.unwrap_err(), LinkPreviewFailure::Cancelled);
    assert!(network.request_cancelled(1));
    network.respond(abandoned, html("New request"));
    assert_eq!(
        service
            .load(abandoned, false, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("New request")
    );
    assert_eq!(
        network
            .requests()
            .iter()
            .filter(|request| request.url == abandoned)
            .count(),
        2
    );
}

#[tokio::test]
async fn active_and_queue_limits_are_shared_and_cancelled_queued_work_never_connects() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let service = network.service(&clock);
    let targets = (0..11)
        .map(|index| format!("https://example.org/queue/{index}"))
        .collect::<Vec<_>>();
    let mut gates = Vec::new();
    for target in targets.iter().take(10) {
        gates.push(network.gated(target, html(target)));
    }
    let tokens = (0..11)
        .map(|_| CancellationToken::new())
        .collect::<Vec<_>>();
    let mut pending = targets
        .iter()
        .zip(&tokens)
        .map(|(target, token)| Box::pin(service.load(target, false, token.clone())))
        .collect::<Vec<_>>();
    for load in pending.iter_mut().take(10) {
        assert_pending(load.as_mut()).await;
    }
    assert_eq!(network.requests().len(), 2);
    assert_eq!(
        pending[10].as_mut().await.unwrap_err(),
        LinkPreviewFailure::Busy
    );
    tokens[2].cancel();
    assert_eq!(
        pending[2].as_mut().await.unwrap_err(),
        LinkPreviewFailure::Cancelled
    );
    let replacement = "https://example.org/replacement";
    network.respond(replacement, html("Replacement"));
    let mut replacement_load = Box::pin(service.load(replacement, false, CancellationToken::new()));
    assert_pending(replacement_load.as_mut()).await;
    gates.remove(0).send(()).unwrap();
    assert!(pending[0].as_mut().await.is_ok());
    for load in pending.iter_mut().take(10).skip(3) {
        assert_pending(load.as_mut()).await;
    }
    assert_eq!(network.requests().len(), 3);
    assert!(
        network
            .requests()
            .iter()
            .all(|request| request.url != targets[2])
    );
    for token in &tokens {
        token.cancel();
    }
    for (index, load) in pending.iter_mut().enumerate().take(10) {
        if index != 0 && index != 2 {
            assert_eq!(
                load.as_mut().await.unwrap_err(),
                LinkPreviewFailure::Cancelled
            );
        }
    }
    drop(pending);
    drop(replacement_load);
    network.respond(&targets[10], html("Busy was not cached"));
    let result = service
        .load(&targets[10], false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.title.as_deref(), Some("Busy was not cached"));
}

#[tokio::test]
async fn cache_evicts_lru_entries_at_the_entry_count_bound() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let service = network.service(&clock);
    for index in 0..64 {
        let target = format!("https://example.org/cache/{index}");
        network.respond(&target, html(&format!("Page {index}")));
        assert!(
            service
                .load(&target, false, CancellationToken::new())
                .await
                .is_ok()
        );
    }
    assert_eq!(
        service
            .load(
                "https://example.org/cache/0",
                false,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Page 0")
    );
    network.respond("https://example.org/cache/64", html("Page 64"));
    assert!(
        service
            .load(
                "https://example.org/cache/64",
                false,
                CancellationToken::new()
            )
            .await
            .is_ok()
    );
    assert_eq!(
        service
            .load(
                "https://example.org/cache/0",
                false,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Page 0")
    );
    network.respond("https://example.org/cache/1", html("Refetched"));
    assert_eq!(
        service
            .load(
                "https://example.org/cache/1",
                false,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Refetched")
    );
    assert_eq!(network.requests().len(), 66);
}

#[tokio::test]
async fn cache_memory_limit_evicts_large_thumbnails_before_the_entry_limit() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let service = network.service(&clock);
    let image = png(512, 288, true);
    assert!(image.len() > 540_000);
    for index in 0..63 {
        let target = format!("https://example.org/weighted/{index}");
        let image_url = format!("https://example.org/weighted/{index}.png");
        network.respond(
            &target,
            Reply::html(format!(
                "<head><title>{index}</title><meta property=og:image content='{image_url}'></head>"
            )),
        );
        network.respond(&image_url, Reply::bytes("image/png", image.clone()));
        assert!(
            service
                .load(&target, false, CancellationToken::new())
                .await
                .unwrap()
                .image
                .is_some()
        );
    }
    network.respond("https://example.org/weighted/0", html("Evicted by weight"));
    assert_eq!(
        service
            .load(
                "https://example.org/weighted/0",
                false,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Evicted by weight")
    );
    assert_eq!(
        network
            .requests()
            .iter()
            .filter(|request| request.url == "https://example.org/weighted/0")
            .count(),
        2
    );
}

#[tokio::test]
async fn public_http_ipv6_and_public_mapped_ipv4_destinations_remain_usable() {
    for (target, host, address, peer) in [
        (
            "http://example.org/plain",
            "example.org",
            "93.184.216.34",
            "93.184.216.34:80",
        ),
        (
            "https://[2606:4700:4700::1111]/page",
            "2606:4700:4700::1111",
            "2606:4700:4700::1111",
            "[2606:4700:4700::1111]:443",
        ),
        (
            "https://[::ffff:93.184.216.34]/page",
            "::ffff:93.184.216.34",
            "::ffff:93.184.216.34",
            "[::ffff:93.184.216.34]:443",
        ),
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        network.answer(host, &[address]);
        network.answer(&format!("[{host}]"), &[address]);
        let mut response = html("Public destination");
        response.peer = peer.parse().unwrap();
        let request_target = if target.contains("::ffff:") {
            "https://[::ffff:5db8:d822]/page"
        } else {
            target
        };
        network.respond(request_target, response);
        let result = network
            .service(&clock)
            .load(target, false, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.title.as_deref(), Some("Public destination"));
        assert_eq!(result.original_target, target);
        assert_eq!(network.requests().len(), 1);
        assert_eq!(
            network.requests()[0].pinned_addresses[0].port(),
            if target.starts_with("http:") { 80 } else { 443 }
        );
    }
}

#[tokio::test]
async fn png_jpeg_webp_and_public_image_redirects_produce_only_sanitized_png_thumbnails() {
    for (format, mime, redirect) in [
        (ImageFormat::Png, "image/png", false),
        (ImageFormat::Jpeg, "image/jpeg", false),
        (ImageFormat::WebP, "image/webp", true),
    ] {
        let network = ScriptedNetwork::default();
        let clock = ManualClock::default();
        let pixels = RgbaImage::from_pixel(160, 90, image::Rgba([80, 120, 160, 255]));
        let source = DynamicImage::ImageRgba8(pixels);
        let source = if format == ImageFormat::Jpeg {
            DynamicImage::ImageRgb8(source.to_rgb8())
        } else {
            source
        };
        let mut output = Cursor::new(Vec::new());
        source.write_to(&mut output, format).unwrap();
        network.respond(PAGE, Reply::html("<head><title>Image formats</title><meta property=og:image content='/cover'></head>"));
        let image_target = if redirect {
            "https://cdn.example.org/final.webp"
        } else {
            "https://example.org/cover"
        };
        if redirect {
            network.respond("https://example.org/cover", Reply::redirect(image_target));
        }
        network.respond(image_target, Reply::bytes(mime, output.into_inner()));
        let result = network
            .service(&clock)
            .load(PAGE, false, CancellationToken::new())
            .await
            .unwrap();
        let image = result.image.unwrap();
        assert_eq!((image.width, image.height), (160, 90));
        assert_eq!(image::guess_format(&image.png).unwrap(), ImageFormat::Png);
        let decoded = image::load_from_memory(&image.png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (160, 90));
        assert_eq!(network.requests().len(), if redirect { 3 } else { 2 });
    }
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    network.respond(PAGE, Reply::html("<head><title>Pixel boundary</title><meta property=og:image content='/boundary.png'></head>"));
    network.respond(
        "https://example.org/boundary.png",
        Reply::bytes("image/png", png(4000, 2000, false)),
    );
    let result = network
        .service(&clock)
        .load(PAGE, false, CancellationToken::new())
        .await
        .unwrap();
    let image = result.image.unwrap();
    assert_eq!((image.width, image.height), (512, 256));
}

#[tokio::test]
async fn an_eight_kibibyte_url_is_accepted_but_one_more_byte_never_connects() {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let prefix = "https://example.org/";
    let allowed = format!("{prefix}{}", "a".repeat(8192 - prefix.len()));
    network.respond(&allowed, html("Boundary URL"));
    let service = network.service(&clock);
    assert_eq!(
        service
            .load(&allowed, false, CancellationToken::new())
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("Boundary URL")
    );
    let rejected = format!("{allowed}a");
    assert!(
        service
            .load(&rejected, false, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(network.requests().len(), 1);
}
