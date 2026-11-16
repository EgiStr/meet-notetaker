//! The LLM providers the user has added and the models they offer, and which
//! one is in use. Plain data kept in a JSON file; API keys are not part of it.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// An OpenAI-compatible endpoint: a vendor's API, a router such as 9router, or a local server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// Without a trailing slash, e.g. `http://localhost:20128/v1`.
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Active {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Providers {
    pub providers: Vec<Provider>,
    pub active: Option<Active>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProvidersError {
    #[error("Nama penyedia tidak boleh kosong.")]
    EmptyName,
    #[error("Alamat harus diawali http:// atau https://.")]
    InvalidUrl,
    #[error("Nama model tidak boleh kosong.")]
    EmptyModel,
    #[error("Penyedia \"{0}\" tidak ada.")]
    UnknownProvider(String),
    #[error("Model \"{0}\" belum ditambahkan ke penyedia ini.")]
    UnknownModel(String),
    #[error("Pengaturan model tidak bisa dibaca atau ditulis: {0}")]
    File(String),
}

impl Providers {
    /// An empty file or none at all means no providers yet.
    pub fn load(path: &Path) -> Result<Self, ProvidersError> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| ProvidersError::File(error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(ProvidersError::File(error.to_string())),
        }
    }

    /// Written to a temporary file first, so that a crash cannot leave half a file.
    pub fn save(&self, path: &Path) -> Result<(), ProvidersError> {
        let file_error = |error: std::io::Error| ProvidersError::File(format!("{}: {error}", path.display()));
        let json = serde_json::to_string_pretty(self).map_err(|error| ProvidersError::File(error.to_string()))?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, json).map_err(file_error)?;
        std::fs::rename(&temporary, path).map_err(file_error)
    }

    /// Adds a provider, or changes the name and address of the one with `id`. Returns its id.
    pub fn upsert(&mut self, id: Option<&str>, name: &str, base_url: &str) -> Result<String, ProvidersError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProvidersError::EmptyName);
        }
        let base_url = base_url.trim().trim_end_matches('/');
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) || base_url.len() < 9 {
            return Err(ProvidersError::InvalidUrl);
        }
        if let Some(id) = id {
            let provider = self.provider_mut(id)?;
            provider.name = name.to_owned();
            provider.base_url = base_url.to_owned();
            return Ok(id.to_owned());
        }
        let id = self.free_id(name);
        self.providers.push(Provider { id: id.clone(), name: name.to_owned(), base_url: base_url.to_owned(), models: Vec::new() });
        Ok(id)
    }

    /// Also stops using it if it was the active provider.
    pub fn remove(&mut self, id: &str) {
        self.providers.retain(|provider| provider.id != id);
        if self.active.as_ref().is_some_and(|active| active.provider == id) {
            self.active = None;
        }
    }

    /// Adds a model unless it is there already. The first model ever added becomes the active one.
    pub fn add_model(&mut self, id: &str, model: &str) -> Result<(), ProvidersError> {
        let model = model.trim();
        if model.is_empty() {
            return Err(ProvidersError::EmptyModel);
        }
        let provider = self.provider_mut(id)?;
        if !provider.models.iter().any(|known| known == model) {
            provider.models.push(model.to_owned());
        }
        if self.active.is_none() {
            self.active = Some(Active { provider: id.to_owned(), model: model.to_owned() });
        }
        Ok(())
    }

    /// Also stops using it if it was the active model.
    pub fn remove_model(&mut self, id: &str, model: &str) -> Result<(), ProvidersError> {
        self.provider_mut(id)?.models.retain(|known| known != model);
        if self.active.as_ref().is_some_and(|active| active.provider == id && active.model == model) {
            self.active = None;
        }
        Ok(())
    }

    pub fn activate(&mut self, id: &str, model: &str) -> Result<(), ProvidersError> {
        if !self.provider_mut(id)?.models.iter().any(|known| known == model) {
            return Err(ProvidersError::UnknownModel(model.to_owned()));
        }
        self.active = Some(Active { provider: id.to_owned(), model: model.to_owned() });
        Ok(())
    }

    /// The provider and model in use, if the user chose one.
    pub fn active_target(&self) -> Option<(&Provider, &str)> {
        let active = self.active.as_ref()?;
        let provider = self.providers.iter().find(|provider| provider.id == active.provider)?;
        Some((provider, active.model.as_str()))
    }

    fn provider_mut(&mut self, id: &str) -> Result<&mut Provider, ProvidersError> {
        self.providers
            .iter_mut()
            .find(|provider| provider.id == id)
            .ok_or_else(|| ProvidersError::UnknownProvider(id.to_owned()))
    }

    /// A readable id from the name, made unique with a number.
    fn free_id(&self, name: &str) -> String {
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if slug.is_empty() { "penyedia".to_owned() } else { slug };
        let mut candidate = base.clone();
        let mut number = 2;
        while self.providers.iter().any(|provider| provider.id == candidate) {
            candidate = format!("{base}-{number}");
            number += 1;
        }
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_router() -> (Providers, String) {
        let mut providers = Providers::default();
        let id = providers.upsert(None, " 9router ", "http://localhost:20128/v1/").unwrap();
        (providers, id)
    }

    #[test]
    fn a_provider_gets_a_readable_unique_id_and_a_clean_address() {
        let (mut providers, id) = with_router();
        assert_eq!(id, "9router");
        assert_eq!(providers.providers[0].base_url, "http://localhost:20128/v1");
        assert_eq!(providers.upsert(None, "9Router", "https://x.example/v1").unwrap(), "9router-2");
        assert_eq!(providers.upsert(None, "Gemini (Google)", "https://g.example").unwrap(), "gemini-google");
    }

    #[test]
    fn names_and_addresses_are_checked() {
        let mut providers = Providers::default();
        assert_eq!(providers.upsert(None, "  ", "http://x.example"), Err(ProvidersError::EmptyName));
        assert_eq!(providers.upsert(None, "A", "localhost:20128"), Err(ProvidersError::InvalidUrl));
        assert_eq!(providers.upsert(Some("tidak-ada"), "A", "http://x.example"), Err(ProvidersError::UnknownProvider("tidak-ada".into())));
        assert!(providers.providers.is_empty());
    }

    #[test]
    fn editing_keeps_the_id_and_the_models() {
        let (mut providers, id) = with_router();
        providers.add_model(&id, "deepseek").unwrap();
        providers.upsert(Some(&id), "Router lokal", "http://localhost:9999/v1").unwrap();
        assert_eq!(providers.providers[0].id, "9router");
        assert_eq!(providers.providers[0].name, "Router lokal");
        assert_eq!(providers.providers[0].models, ["deepseek"]);
    }

    #[test]
    fn the_first_model_becomes_active_and_others_must_be_chosen() {
        let (mut providers, id) = with_router();
        providers.add_model(&id, "deepseek").unwrap();
        providers.add_model(&id, "deepseek").unwrap();
        providers.add_model(&id, " qwen ").unwrap();
        assert_eq!(providers.providers[0].models, ["deepseek", "qwen"]);
        assert_eq!(providers.active_target().map(|(_, model)| model), Some("deepseek"));

        providers.activate(&id, "qwen").unwrap();
        assert_eq!(providers.active_target().map(|(_, model)| model), Some("qwen"));
        assert_eq!(providers.activate(&id, "lain"), Err(ProvidersError::UnknownModel("lain".into())));
        assert_eq!(providers.add_model(&id, " "), Err(ProvidersError::EmptyModel));
    }

    #[test]
    fn removing_the_active_model_or_provider_leaves_nothing_active() {
        let (mut providers, id) = with_router();
        providers.add_model(&id, "deepseek").unwrap();
        providers.add_model(&id, "qwen").unwrap();
        providers.remove_model(&id, "qwen").unwrap();
        assert!(providers.active.is_some());
        providers.remove_model(&id, "deepseek").unwrap();
        assert!(providers.active.is_none());

        providers.add_model(&id, "x").unwrap();
        providers.remove(&id);
        assert!(providers.providers.is_empty() && providers.active.is_none());
    }

    #[test]
    fn settings_survive_a_save_and_a_missing_file_is_empty() {
        let path = std::env::temp_dir().join(format!("providers-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(Providers::load(&path).unwrap(), Providers::default());

        let (mut providers, id) = with_router();
        providers.add_model(&id, "deepseek").unwrap();
        providers.save(&path).unwrap();
        assert_eq!(Providers::load(&path).unwrap(), providers);

        std::fs::write(&path, "bukan json").unwrap();
        assert!(matches!(Providers::load(&path), Err(ProvidersError::File(_))));
        let _ = std::fs::remove_file(&path);
    }
}
