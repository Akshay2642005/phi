use rig::providers::ollama;

pub struct RigRuntime {
    pub client: ollama::Client,
    pub model: String,
}

impl RigRuntime {
    pub fn new(api_key: String, base_url: String, model: String) -> anyhow::Result<Self> {
        let client = ollama::Client::builder()
            .api_key(api_key)
            .base_url(base_url)
            .build()?;

        Ok(Self { client, model })
    }
}
