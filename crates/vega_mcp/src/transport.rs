pub(crate) trait SendRequest {
    async fn send_mcp(self) -> Result<reqwest::Response, reqwest::Error>;
}
impl SendRequest for reqwest::RequestBuilder {
    async fn send_mcp(self) -> Result<reqwest::Response, reqwest::Error> {
        let (client, request) = self.build_split();
        let request = request?;
        #[cfg(any(test, feature = "test-support"))]
        {
            let _ = client;
            crate::mock::send(request).await
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            client.execute(request).await
        }
    }
}
