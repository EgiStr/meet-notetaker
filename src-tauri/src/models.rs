//! Choosing the LLM: the providers and models the user added, kept in a JSON
//! file in the app data folder, with each provider's API key in the Windows
//! Credential Manager. Without a chosen model the `LLM_*` variables still work.

use std::sync::Arc;

use notetaker_core::embed::{default_model, EmbedError, OpenAiEmbedder};
use notetaker_core::llm::{LlmClient, LlmError, Prompt};
use notetaker_core::openai_llm::{list_models, OpenAiLlm, OpenAiLlmSettings};
use notetaker_core::providers::{Active, Providers};
use tauri::async_runtime::spawn_blocking;
use tauri::{AppHandle, Manager};

const KEY_SERVICE: &str = "meet-notetaker";

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    Ok(dir.join("providers.json"))
}

fn load(app: &AppHandle) -> Result<Providers, String> {
    Providers::load(&settings_path(app)?).map_err(|error| error.to_string())
}

fn save(app: &AppHandle, providers: &Providers) -> Result<(), String> {
    providers.save(&settings_path(app)?).map_err(|error| error.to_string())
}

fn key_entry(provider_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEY_SERVICE, provider_id).map_err(|error| error.to_string())
}

/// `None` when no key was stored (a local server may not need one).
fn stored_key(provider_id: &str) -> Result<Option<String>, String> {
    match key_entry(provider_id)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Kunci API tidak bisa dibaca dari Credential Manager: {error}")),
    }
}

fn forget_key(provider_id: &str) -> Result<(), String> {
    match key_entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// What the screen shows: never the keys themselves, only whether one is stored.
#[derive(serde::Serialize)]
pub struct ProviderView {
    id: String,
    name: String,
    base_url: String,
    models: Vec<String>,
    has_key: bool,
}

#[derive(serde::Serialize)]
pub struct ModelsView {
    providers: Vec<ProviderView>,
    active: Option<Active>,
    /// True while no model is chosen and the `LLM_*` variables are used instead.
    env_fallback: bool,
}

fn view(providers: &Providers) -> Result<ModelsView, String> {
    let mut shown = Vec::new();
    for provider in &providers.providers {
        shown.push(ProviderView {
            id: provider.id.clone(),
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            models: provider.models.clone(),
            has_key: stored_key(&provider.id)?.is_some(),
        });
    }
    Ok(ModelsView {
        providers: shown,
        active: providers.active.clone(),
        env_fallback: providers.active_target().is_none() && std::env::var("LLM_API_KEY").is_ok(),
    })
}

/// Reads the settings again on every call, so a model chosen in the middle of a
/// meeting is used from the next request on.
pub struct SettingsLlm(pub AppHandle);

impl LlmClient for SettingsLlm {
    fn complete(&self, prompt: &Prompt) -> Result<String, LlmError> {
        current_llm(&self.0)?.complete(prompt)
    }
}

fn current_llm(app: &AppHandle) -> Result<Box<dyn LlmClient>, LlmError> {
    Ok(Box::new(OpenAiLlm::new(current_settings(app)?)?))
}

/// The address, key and model of the chosen provider, or of the `LLM_*` variables.
fn current_settings(app: &AppHandle) -> Result<OpenAiLlmSettings, LlmError> {
    let not_configured = |reason: String| LlmError::NotConfigured(reason);
    let providers = load(app).map_err(not_configured)?;
    match providers.active_target() {
        Some((provider, model)) => Ok(OpenAiLlmSettings {
            base_url: provider.base_url.clone(),
            api_key: stored_key(&provider.id).map_err(not_configured)?.unwrap_or_default(),
            model: model.to_owned(),
        }),
        None => env_settings(),
    }
}

/// Embeds with the provider named by `EMBEDDING_BASE_URL` and `EMBEDDING_API_KEY` (Gemini when the
/// address is left out), or else with the chosen LLM provider. The model is `EMBEDDING_MODEL`, or
/// the usual one of well-known providers; for others, embeddings stay off until it is set.
pub fn embedder(app: &AppHandle) -> Result<OpenAiEmbedder, EmbedError> {
    let (base_url, api_key) = match std::env::var("EMBEDDING_API_KEY") {
        Ok(key) => (
            std::env::var("EMBEDDING_BASE_URL").unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta/openai".into()),
            key,
        ),
        Err(_) => {
            let settings = current_settings(app).map_err(|error| EmbedError::NotConfigured(error.to_string()))?;
            (settings.base_url, settings.api_key)
        }
    };
    let model = std::env::var("EMBEDDING_MODEL")
        .ok()
        .or_else(|| default_model(&base_url).map(str::to_owned))
        .ok_or_else(|| EmbedError::NotConfigured("isi EMBEDDING_MODEL untuk penyedia ini".into()))?;
    OpenAiEmbedder::new(&base_url, &api_key, &model)
}

fn env_settings() -> Result<OpenAiLlmSettings, LlmError> {
    let api_key = std::env::var("LLM_API_KEY").map_err(|_| {
        LlmError::NotConfigured("belum ada model dipilih; buka Model untuk menambahkan penyedia dan model".into())
    })?;
    let env_or = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
    Ok(OpenAiLlmSettings {
        base_url: env_or("LLM_BASE_URL", "https://generativelanguage.googleapis.com/v1beta/openai"),
        api_key,
        model: env_or("LLM_MODEL", "gemini-2.5-flash"),
    })
}

pub fn llm(app: &AppHandle) -> Arc<dyn LlmClient> {
    Arc::new(SettingsLlm(app.clone()))
}

#[tauri::command]
pub fn get_models(app: AppHandle) -> Result<ModelsView, String> {
    view(&load(&app)?)
}

/// Adds a provider (`id` empty) or edits one. A blank `api_key` keeps the stored key.
#[tauri::command]
pub fn save_provider(id: Option<String>, name: String, base_url: String, api_key: String, app: AppHandle) -> Result<ModelsView, String> {
    let mut providers = load(&app)?;
    let id = providers.upsert(id.as_deref(), &name, &base_url).map_err(|error| error.to_string())?;
    if !api_key.trim().is_empty() {
        key_entry(&id)?.set_password(api_key.trim()).map_err(|error| format!("Kunci API tidak bisa disimpan: {error}"))?;
    }
    save(&app, &providers)?;
    view(&providers)
}

#[tauri::command]
pub fn remove_provider(id: String, app: AppHandle) -> Result<ModelsView, String> {
    let mut providers = load(&app)?;
    providers.remove(&id);
    forget_key(&id)?;
    save(&app, &providers)?;
    view(&providers)
}

#[tauri::command]
pub fn add_model(id: String, model: String, app: AppHandle) -> Result<ModelsView, String> {
    let mut providers = load(&app)?;
    providers.add_model(&id, &model).map_err(|error| error.to_string())?;
    save(&app, &providers)?;
    view(&providers)
}

#[tauri::command]
pub fn remove_model(id: String, model: String, app: AppHandle) -> Result<ModelsView, String> {
    let mut providers = load(&app)?;
    providers.remove_model(&id, &model).map_err(|error| error.to_string())?;
    save(&app, &providers)?;
    view(&providers)
}

#[tauri::command]
pub fn activate_model(id: String, model: String, app: AppHandle) -> Result<ModelsView, String> {
    let mut providers = load(&app)?;
    providers.activate(&id, &model).map_err(|error| error.to_string())?;
    save(&app, &providers)?;
    view(&providers)
}

/// The model ids the provider says it has, to pick from instead of typing.
#[tauri::command]
pub async fn fetch_models(id: String, app: AppHandle) -> Result<Vec<String>, String> {
    spawn_blocking(move || {
        let providers = load(&app)?;
        let provider = providers
            .providers
            .iter()
            .find(|provider| provider.id == id)
            .ok_or_else(|| format!("Penyedia \"{id}\" tidak ada."))?;
        let key = stored_key(&provider.id)?.unwrap_or_default();
        list_models(&provider.base_url, &key).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Sends one tiny request through the model, to see that address, key and name work.
#[tauri::command]
pub async fn test_model(id: String, model: String, app: AppHandle) -> Result<String, String> {
    spawn_blocking(move || {
        let providers = load(&app)?;
        let provider = providers
            .providers
            .iter()
            .find(|provider| provider.id == id)
            .ok_or_else(|| format!("Penyedia \"{id}\" tidak ada."))?;
        let settings = OpenAiLlmSettings {
            base_url: provider.base_url.clone(),
            api_key: stored_key(&provider.id)?.unwrap_or_default(),
            model,
        };
        let prompt = Prompt { system: "Jawab sesingkat mungkin.".into(), user: "Balas dengan satu kata: siap".into() };
        OpenAiLlm::new(settings).and_then(|llm| llm.complete(&prompt)).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
