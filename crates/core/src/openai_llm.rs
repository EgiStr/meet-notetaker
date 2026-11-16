//! LLM client for any endpoint that implements OpenAI's
//! `POST /chat/completions` (Gemini, Groq, OpenAI, and compatible servers).

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

use crate::llm::{LlmClient, LlmError, Prompt};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Routers list hundreds of models, which can take a while.
const MODELS_TIMEOUT: Duration = Duration::from_secs(60);
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

/// The model ids a provider offers (`GET {base_url}/models`), sorted.
pub fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, LlmError> {
    let client = Client::builder().timeout(MODELS_TIMEOUT).build()?;
    let mut request = client.get(format!("{base_url}/models"));
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    let response = request.send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(LlmError::Rejected { status: status.as_u16(), body: error_message(&body) });
    }
    parse_models(&body)
}

fn parse_models(body: &str) -> Result<Vec<String>, LlmError> {
    #[derive(Deserialize)]
    struct List {
        #[serde(default)]
        data: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        id: String,
    }
    let list: List = serde_json::from_str(body)?;
    let mut ids: Vec<String> = list.data.into_iter().map(|entry| entry.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
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
    // Some gateways (9router) answer as an event stream even without streaming: the
    // JSON, optionally behind "data:", is followed by "data: [DONE]". Read the first value only.
    let json = body.trim().strip_prefix("data:").unwrap_or(body).trim_start();
    let response: Response = serde_json::Deserializer::from_str(json).into_iter().next().ok_or(LlmError::Empty)??;
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
    fn models_are_listed_sorted_without_repeats() {
        let body = r#"{"object":"list","data":[{"id":"b","object":"model"},{"id":"a"},{"id":"b"}]}"#;
        assert_eq!(parse_models(body).unwrap(), ["a", "b"]);
        assert!(parse_models(r#"{"object":"list"}"#).unwrap().is_empty());
        assert!(parse_models("<html>").is_err());
    }

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
    fn an_event_stream_trailer_after_the_json_is_ignored() {
        let json = r#"{"choices":[{"message":{"content":"halo"}}]}"#;
        assert_eq!(parse_completion(&format!("{json}data: [DONE]\n\ndata: [DONE]")).unwrap(), "halo");
        assert_eq!(parse_completion(&format!("data: {json}\n\ndata: [DONE]\n")).unwrap(), "halo");
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
