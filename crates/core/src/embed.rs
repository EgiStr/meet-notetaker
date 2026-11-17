//! Text embeddings, for finding what a meeting said by meaning instead of by
//! the words of the question. Any endpoint with OpenAI's `POST /embeddings` works.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding belum diatur: {0}")]
    NotConfigured(String),
    #[error("permintaan embedding gagal: {0}")]
    Request(#[from] reqwest::Error),
    #[error("penyedia menolak embedding (HTTP {status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("jawaban embedding tidak bisa dibaca: {0}")]
    Malformed(String),
}

pub trait Embedder: Send + Sync {
    /// The model that made the vectors; vectors of different models cannot be compared.
    fn model(&self) -> &str;
    /// One vector per text, in the same order.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
}

/// The embedding model to use with `base_url` when the user did not name one.
/// `None` for providers whose embedding model is not known.
pub fn default_model(base_url: &str) -> Option<&'static str> {
    if base_url.contains("generativelanguage.googleapis.com") {
        Some("gemini-embedding-001")
    } else if base_url.contains("api.openai.com") {
        Some("text-embedding-3-small")
    } else {
        None
    }
}

pub struct OpenAiEmbedder {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    /// Shorter vectors take less space and are as good for search; only for providers known to accept it.
    dimensions: Option<usize>,
}

const VECTOR_DIMENSIONS: usize = 768;

/// Texts per request; providers limit the batch size.
const BATCH: usize = 32;

impl OpenAiEmbedder {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Result<Self, EmbedError> {
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
        let dimensions = default_model(base_url).map(|_| VECTOR_DIMENSIONS);
        Ok(Self { client, base_url: base_url.trim_end_matches('/').to_owned(), api_key: api_key.to_owned(), model: model.to_owned(), dimensions })
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut body = json!({ "model": self.model, "input": texts });
        if let Some(dimensions) = self.dimensions {
            body["dimensions"] = json!(dimensions);
        }
        let mut request = self.client.post(format!("{}/embeddings", self.base_url)).json(&body);
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }
        let response = request.send()?;
        let status = response.status();
        let body = response.text()?;
        if !status.is_success() {
            return Err(EmbedError::Rejected { status: status.as_u16(), body: body.chars().take(300).collect() });
        }
        parse_embeddings(&body, texts.len())
    }
}

impl Embedder for OpenAiEmbedder {
    fn model(&self) -> &str {
        &self.model
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            vectors.extend(self.embed_batch(batch)?);
        }
        Ok(vectors)
    }
}

fn parse_embeddings(body: &str, expected: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
    #[derive(Deserialize)]
    struct Response {
        data: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        #[serde(default)]
        index: usize,
        embedding: Vec<f32>,
    }
    // Gateways may answer as an event stream: the JSON behind an optional "data:".
    let json = body.trim().strip_prefix("data:").unwrap_or(body).trim_start();
    let response: Response = serde_json::Deserializer::from_str(json)
        .into_iter()
        .next()
        .ok_or_else(|| EmbedError::Malformed("kosong".into()))?
        .map_err(|error| EmbedError::Malformed(error.to_string()))?;
    let mut entries = response.data;
    entries.sort_by_key(|entry| entry.index);
    if entries.len() != expected || entries.iter().any(|entry| entry.embedding.is_empty()) {
        return Err(EmbedError::Malformed(format!("diharapkan {expected} vektor, diterima {}", entries.len())));
    }
    Ok(entries.into_iter().map(|entry| entry.embedding).collect())
}

/// 1 for the same direction, 0 for unrelated, -1 for opposite. 0 for vectors that cannot be compared.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let (na, nb) = (norm(a), norm(b));
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

pub fn to_bytes(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|value| value.to_le_bytes()).collect()
}

pub fn from_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_come_back_in_input_order_even_when_the_provider_shuffles_them() {
        let body = r#"{"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}"#;
        assert_eq!(parse_embeddings(body, 2).unwrap(), [vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn an_event_stream_wrapper_and_a_wrong_count_are_handled() {
        let body = "data: {\"data\":[{\"embedding\":[0.5]}]}\n\ndata: [DONE]";
        assert_eq!(parse_embeddings(body, 1).unwrap(), [vec![0.5]]);
        assert!(parse_embeddings(body, 2).is_err());
        assert!(parse_embeddings("<html>", 1).is_err());
    }

    #[test]
    fn cosine_measures_direction_not_length() {
        assert!((cosine(&[1.0, 0.0], &[3.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 2.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn vectors_survive_storage_as_bytes() {
        let vector = vec![0.25, -1.5, 3.0];
        assert_eq!(from_bytes(&to_bytes(&vector)), vector);
    }

    #[test]
    fn known_providers_have_a_default_model() {
        assert_eq!(default_model("https://generativelanguage.googleapis.com/v1beta/openai"), Some("gemini-embedding-001"));
        assert_eq!(default_model("http://localhost:20128/v1"), None);
    }
}
