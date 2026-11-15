//! LLM client for any endpoint that implements OpenAI's
//! `POST /chat/completions` (Gemini, Groq, OpenAI, and compatible servers).

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

use crate::llm::{LlmClient, LlmError, Prompt};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Low, so that minutes stay close to what was said.
const TEMPERATURE: f64 = 0.2;

#[derive(Debug, Clone)]
pub struct OpenAiLlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub struct OpenAiLlm {
    client: Client,
    settings: OpenAiLlmSettings,
}

impl OpenAiLlm {
    pub fn new(settings: OpenAiLlmSettings) -> Result<Self, LlmError> {
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
        Ok(Self { client, settings })
    }
}

impl LlmClient for OpenAiLlm {
    fn complete(&self, prompt: &Prompt) -> Result<String, LlmError> {
        let request = json!({
            "model": self.settings.model,
            "temperature": TEMPERATURE,
            "messages": [
                { "role": "system", "content": prompt.system },
                { "role": "user", "content": prompt.user },
            ],
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", self.settings.base_url))
            .bearer_auth(&self.settings.api_key)
            .json(&request)
            .send()?;

        let status = response.status();
        let body = response.text()?;
        if !status.is_success() {
            return Err(LlmError::Rejected { status: status.as_u16(), body: error_message(&body) });
        }
        parse_completion(&body)
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: Option<String>,
}

fn parse_completion(body: &str) -> Result<String, LlmError> {
    let response: Response = serde_json::from_str(body)?;
    response
        .choices
        .into_iter()
        .find_map(|choice| choice.message.content)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .ok_or(LlmError::Empty)
}

/// The provider's own explanation, which is what the user is shown. Providers
/// send `{"error":{"message":…}}`; Gemini wraps that object in an array.
fn error_message(body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|value| {
        let object = value.get(0).unwrap_or(value);
        object.get("error")?.get("message")?.as_str()
    });
    message.unwrap_or(body).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_is_taken_from_an_openai_style_body() {
        assert_eq!(error_message(r#"{"error":{"message":"Invalid API key","code":401}}"#), "Invalid API key");
    }

    #[test]
    fn error_message_is_taken_from_a_gemini_style_array() {
        assert_eq!(error_message(r#"[{"error":{"code":429,"message":"Quota exceeded"}}]"#), "Quota exceeded");
    }

    #[test]
    fn unrecognized_error_body_is_kept_as_is() {
        assert_eq!(error_message("Bad Gateway\n"), "Bad Gateway");
    }

    #[test]
    fn text_of_the_first_choice_is_returned_trimmed() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"\n## Ringkasan\n- Halo\n"}}]}"#;
        assert_eq!(parse_completion(body).unwrap(), "## Ringkasan\n- Halo");
    }

    #[test]
    fn response_without_choices_is_empty() {
        assert!(matches!(parse_completion(r#"{"choices":[]}"#), Err(LlmError::Empty)));
    }

    #[test]
    fn null_or_blank_content_is_empty() {
        let null = r#"{"choices":[{"message":{"content":null}}]}"#;
        let blank = r#"{"choices":[{"message":{"content":"  "}}]}"#;
        assert!(matches!(parse_completion(null), Err(LlmError::Empty)));
        assert!(matches!(parse_completion(blank), Err(LlmError::Empty)));
    }

    #[test]
    fn invalid_json_is_reported_as_malformed() {
        assert!(matches!(parse_completion("not json"), Err(LlmError::Malformed(_))));
    }
}
