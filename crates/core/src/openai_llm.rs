//! LLM client for any endpoint that implements OpenAI's
//! `POST /chat/completions` (Gemini, Groq, OpenAI, and compatible servers).

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

use crate::llm::{LlmClient, LlmError, Prompt};
use crate::retry::{is_transient_status, with_retries, PATIENT_DELAYS};

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
        let client = Client::builder().timeout(REQUEST_TIMEOUT).pool_idle_timeout(Duration::from_secs(10)).build()?;
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

        // A provider that is busy, or a connection that drops, usually works a moment later.
        with_retries(PATIENT_DELAYS, is_transient, || {
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
        })
    }
}

fn is_transient(error: &LlmError) -> bool {
    match error {
        LlmError::Http(_) | LlmError::Malformed(_) | LlmError::Empty => true,
        LlmError::Rejected { status, .. } => is_transient_status(*status),
        LlmError::NotConfigured(_) => false,
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
    /// Absent when the model stopped before writing anything (its token limit, a content filter).
    #[serde(default)]
    message: Option<Message>,
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
        .find_map(|choice| choice.message?.content)
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
    fn a_choice_without_a_message_is_empty_not_malformed() {
        let body = r#"{"choices":[{"index":0,"finish_reason":"length"}]}"#;
        assert!(matches!(parse_completion(body), Err(LlmError::Empty)));
    }

    #[test]
    fn the_next_choice_is_used_when_the_first_has_no_message() {
        let body = r#"{"choices":[{"finish_reason":"length"},{"message":{"content":"halo"}}]}"#;
        assert_eq!(parse_completion(body).unwrap(), "halo");
    }

    #[test]
    fn only_passing_trouble_is_retried() {
        assert!(is_transient(&LlmError::Empty));
        assert!(is_transient(&LlmError::Rejected { status: 503, body: String::new() }));
        assert!(!is_transient(&LlmError::Rejected { status: 401, body: String::new() }));
        assert!(!is_transient(&LlmError::NotConfigured(String::new())));
    }

    #[test]
    fn null_or_blank_content_is_empty() {
        let null = r#"{"choices":[{"message":{"content":null}}]}"#;
        let blank = r#"{"choices":[{"message":{"content":"  "}}]}"#;
        assert!(matches!(parse_completion(null), Err(LlmError::Empty)));
        assert!(matches!(parse_completion(blank), Err(LlmError::Empty)));
    }

    /// Serves the given raw HTTP responses, one per connection, and returns the base URL.
    fn serve(responses: Vec<String>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://{address}")
    }

    fn http(status: &str, body: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn llm_at(base_url: String) -> OpenAiLlm {
        OpenAiLlm::new(OpenAiLlmSettings { base_url, api_key: "k".into(), model: "m".into() }).unwrap()
    }

    fn prompt() -> Prompt {
        Prompt { system: "s".into(), user: "u".into() }
    }

    #[test]
    fn a_busy_provider_is_asked_again() {
        let ok = r#"{"choices":[{"message":{"content":"halo"}}]}"#;
        let base = serve(vec![
            http("503 Service Unavailable", r#"{"error":{"message":"busy"}}"#),
            http("200 OK", r#"{"choices":[{"finish_reason":"length"}]}"#),
            http("200 OK", ok),
        ]);
        assert_eq!(llm_at(base).complete(&prompt()).unwrap(), "halo");
    }

    #[test]
    fn a_rejected_key_is_reported_at_once() {
        let base = serve(vec![http("401 Unauthorized", r#"{"error":{"message":"Invalid API key"}}"#)]);
        let error = llm_at(base).complete(&prompt()).unwrap_err();
        assert!(matches!(error, LlmError::Rejected { status: 401, .. }), "{error}");
    }

    #[test]
    fn invalid_json_is_reported_as_malformed() {
        assert!(matches!(parse_completion("not json"), Err(LlmError::Malformed(_))));
    }
}
