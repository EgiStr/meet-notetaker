//! Types shared by every text-generation provider.

#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("LLM is not configured: {0}")]
    NotConfigured(String),
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("unexpected response: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("provider returned no text")]
    Empty,
}

pub trait LlmClient: Send + Sync {
    fn complete(&self, prompt: &Prompt) -> Result<String, LlmError>;
}
