//! The transcription service the user chose in the app: which provider, where it lives and which
//! models to use. Plain data in a JSON file; the API key is kept elsewhere (Credential Manager).

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const GROQ_URL: &str = "https://api.groq.com/openai/v1";
const GROQ_LIVE_MODEL: &str = "whisper-large-v3-turbo";
const GROQ_FINAL_MODEL: &str = "whisper-large-v3";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SttSettings {
    /// "openai" (any OpenAI-compatible endpoint) or "deepgram".
    pub provider: String,
    /// Only for "openai"; without a trailing slash.
    pub base_url: String,
    /// The model used while recording, where speed matters.
    pub model: String,
    /// The model used over the finished recording; empty means the usual choice for the provider.
    #[serde(default)]
    pub final_model: String,
}

impl Default for SttSettings {
    fn default() -> Self {
        Self { provider: "openai".into(), base_url: GROQ_URL.into(), model: GROQ_LIVE_MODEL.into(), final_model: String::new() }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SttError {
    #[error("Penyedia transkripsi harus \"openai\" atau \"deepgram\".")]
    UnknownProvider,
    #[error("Alamat harus diawali http:// atau https://.")]
    InvalidUrl,
    #[error("Nama model tidak boleh kosong.")]
    EmptyModel,
    #[error("Pengaturan transkripsi tidak bisa dibaca atau ditulis: {0}")]
    File(String),
}

impl SttSettings {
    /// A missing file means nothing was chosen yet.
    pub fn load(path: &Path) -> Result<Option<Self>, SttError> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| SttError::File(error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SttError::File(error.to_string())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SttError> {
        let file_error = |error: std::io::Error| SttError::File(format!("{}: {error}", path.display()));
        let json = serde_json::to_string_pretty(self).map_err(|error| SttError::File(error.to_string()))?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, json).map_err(file_error)?;
        std::fs::rename(&temporary, path).map_err(file_error)
    }

    /// Trims the fields and refuses what cannot work.
    pub fn validated(mut self) -> Result<Self, SttError> {
        self.provider = self.provider.trim().to_owned();
        self.base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        self.model = self.model.trim().to_owned();
        self.final_model = self.final_model.trim().to_owned();
        match self.provider.as_str() {
            "openai" if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) => Err(SttError::InvalidUrl),
            "openai" | "deepgram" if self.model.is_empty() => Err(SttError::EmptyModel),
            "openai" | "deepgram" => Ok(self),
            _ => Err(SttError::UnknownProvider),
        }
    }

    /// The model for the live transcript, or for the pass over the finished recording. Groq's full
    /// model is clearly more accurate than the turbo one, for the cost of time.
    pub fn model_for(&self, final_pass: bool) -> &str {
        if !final_pass {
            return &self.model;
        }
        if !self.final_model.is_empty() {
            return &self.final_model;
        }
        if self.provider == "openai" && self.base_url.contains("groq.com") && self.model == GROQ_LIVE_MODEL {
            return GROQ_FINAL_MODEL;
        }
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_final_pass_uses_its_own_model_the_groq_full_model_or_the_live_one() {
        let mut settings = SttSettings::default();
        assert_eq!(settings.model_for(false), "whisper-large-v3-turbo");
        assert_eq!(settings.model_for(true), "whisper-large-v3");
        settings.final_model = "whisper-1".into();
        assert_eq!(settings.model_for(true), "whisper-1");
        let other = SttSettings { base_url: "https://api.openai.com/v1".into(), model: "whisper-1".into(), ..SttSettings::default() };
        assert_eq!(other.model_for(true), "whisper-1");
    }

    #[test]
    fn settings_are_checked_and_cleaned() {
        let ok = SttSettings { base_url: " https://x.test/v1/ ".into(), model: " m ".into(), ..SttSettings::default() };
        let ok = ok.validated().unwrap();
        assert_eq!((ok.base_url.as_str(), ok.model.as_str()), ("https://x.test/v1", "m"));
        assert_eq!(SttSettings { base_url: "x.test".into(), ..SttSettings::default() }.validated(), Err(SttError::InvalidUrl));
        assert_eq!(SttSettings { model: " ".into(), ..SttSettings::default() }.validated(), Err(SttError::EmptyModel));
        assert_eq!(SttSettings { provider: "x".into(), ..SttSettings::default() }.validated(), Err(SttError::UnknownProvider));
        let deepgram = SttSettings { provider: "deepgram".into(), base_url: String::new(), model: "nova-2".into(), final_model: String::new() };
        assert!(deepgram.validated().is_ok());
    }

    #[test]
    fn settings_survive_a_round_trip_and_a_missing_file_means_none() {
        let dir = std::env::temp_dir().join("notetaker-stt-settings-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stt.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(SttSettings::load(&path).unwrap(), None);
        let settings = SttSettings { final_model: "whisper-1".into(), ..SttSettings::default() };
        settings.save(&path).unwrap();
        assert_eq!(SttSettings::load(&path).unwrap(), Some(settings));
    }
}
