//! Choosing the transcription service in the app: its settings in a JSON file in the app data
//! folder and its API key in the Windows Credential Manager. Without a stored key the `STT_*`
//! environment variables are used, as before.

use notetaker_core::openai_llm::list_models;
use notetaker_core::stt_settings::SttSettings;
use tauri::async_runtime::spawn_blocking;
use tauri::{AppHandle, Manager};

const KEY_SERVICE: &str = "meet-notetaker";
const KEY_ACCOUNT: &str = "stt";

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("stt.json"))
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEY_SERVICE, KEY_ACCOUNT).map_err(|error| error.to_string())
}

fn stored_key() -> Result<Option<String>, String> {
    match key_entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Kunci API tidak bisa dibaca dari Credential Manager: {error}")),
    }
}

/// The settings and key chosen in the app, if any; `None` means the environment decides.
pub fn chosen(app: &AppHandle) -> Result<Option<(SttSettings, String)>, String> {
    let Some(settings) = SttSettings::load(&settings_path(app)?).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    Ok(stored_key()?.map(|key| (settings, key)))
}

/// What the screen shows: never the key itself.
#[derive(serde::Serialize)]
pub struct SttView {
    settings: SttSettings,
    has_key: bool,
    /// Nothing is stored and the STT_* variables are in use (or transcription is not set up at all).
    env_fallback: bool,
}

fn view(app: &AppHandle) -> Result<SttView, String> {
    let stored = SttSettings::load(&settings_path(app)?).map_err(|error| error.to_string())?;
    let has_key = stored_key()?.is_some();
    Ok(SttView { env_fallback: !(stored.is_some() && has_key), settings: stored.unwrap_or_default(), has_key })
}

#[tauri::command]
pub fn get_stt(app: AppHandle) -> Result<SttView, String> {
    view(&app)
}

/// A blank `api_key` keeps the stored one.
#[tauri::command]
pub fn save_stt(settings: SttSettings, api_key: String, app: AppHandle) -> Result<SttView, String> {
    let settings = settings.validated().map_err(|error| error.to_string())?;
    if !api_key.trim().is_empty() {
        key_entry()?.set_password(api_key.trim()).map_err(|error| format!("Kunci API tidak bisa disimpan: {error}"))?;
    }
    settings.save(&settings_path(&app)?).map_err(|error| error.to_string())?;
    view(&app)
}

/// Forgets the choice made in the app, so that the STT_* variables apply again.
#[tauri::command]
pub fn reset_stt(app: AppHandle) -> Result<SttView, String> {
    match std::fs::remove_file(settings_path(&app)?) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    match key_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(error.to_string()),
    }
    view(&app)
}

/// Asks the service for its models, which shows that the address and key are accepted.
#[tauri::command]
pub async fn test_stt(app: AppHandle) -> Result<String, String> {
    spawn_blocking(move || {
        let (settings, key) = chosen(&app)?.ok_or("Simpan pengaturan dan kunci dulu.")?;
        if settings.provider != "openai" {
            return Err("Pengujian hanya tersedia untuk penyedia kompatibel OpenAI; coba rekam sebentar.".to_owned());
        }
        let models = list_models(&settings.base_url, &key).map_err(|error| error.to_string())?;
        Ok(format!("Terhubung; {} model tersedia.", models.len()))
    })
    .await
    .map_err(|error| error.to_string())?
}
