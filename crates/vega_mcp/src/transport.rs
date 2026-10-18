pub(crate) trait SendRequest {
    async fn send_mcp(self) -> Result<reqwest::Response, reqwest::Error>;
}
impl SendRequest for reqwest::RequestBuilder {
    async fn send_mcp(self) -> Result<reqwest::Response, reqwest::Error> {
        let (client, request) = self.build_split();
        let request = request?;

        client.execute(request).await
    }
}
