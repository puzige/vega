use super::*;

#[derive(Default)]
struct FailingResolver {
    calls: AtomicUsize,
}

impl reqwest::dns::Resolve for FailingResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        assert_eq!(name.as_str(), "fixture.invalid");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "fixture DNS unavailable",
            )
            .into())
        })
    }
}
