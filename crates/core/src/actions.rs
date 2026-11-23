//! Action items: who has to do what by when, taken from a meeting, and sent on to other tools.

use std::time::Duration;

use crate::llm::Prompt;
use crate::prompts::format_transcript;
use crate::speakers::SpeakerNames;
use crate::transcript::TranscriptSegment;

/// What the LLM found in one meeting.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActionDraft {
    pub task: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
}

/// An action item as it is kept, with the meeting it came from.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ActionItem {
    pub id: i64,
    pub meeting_id: i64,
    pub meeting_title: String,
    pub task: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub done: bool,
}

const SYSTEM: &str = "\
Anda mengambil daftar tindak lanjut (action item) dari sebuah rapat. Balas HANYA dengan larik JSON, \
tanpa teks lain: [{\"task\": \"...\", \"owner\": \"...\", \"due\": \"...\"}]. \"task\" adalah \
pekerjaan yang konkret, satu kalimat pendek. \"owner\" adalah nama orang yang bertanggung jawab \
persis seperti di transkrip (\"Saya\" untuk pengguna), \"due\" tenggat yang disebut (tanggal bila \
ada, kalau tidak teks aslinya seperti \"Jumat\"). Isi \"owner\" dan \"due\" dengan null bila tidak \
disebut; jangan mengarang. Hanya pekerjaan yang benar-benar disepakati, bukan topik diskusi. \
Larik kosong [] bila tidak ada.";

pub fn prompt(mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let mom = if mom.trim().is_empty() { "(belum ada)" } else { mom };
    Prompt {
        system: SYSTEM.into(),
        user: format!("MoM:\n{mom}\n\nTranskrip:\n{}\n\nDaftar action item (JSON):", format_transcript(segments, names)),
    }
}

/// The items in an LLM reply: the JSON array, wherever in the text it sits. Blank tasks and
/// placeholder owners and dues are dropped.
pub fn parse(reply: &str) -> Vec<ActionDraft> {
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return Vec::new();
    };
    let Some(json) = reply.get(start..=end) else { return Vec::new() };
    let drafts: Vec<ActionDraft> = serde_json::from_str(json).unwrap_or_default();
    let clean = |value: Option<String>| {
        value.map(|text| text.trim().to_owned()).filter(|text| {
            let lower = text.to_lowercase();
            !text.is_empty() && !matches!(lower.as_str(), "null" | "-" | "n/a" | "(belum ditentukan)" | "belum ditentukan")
        })
    };
    drafts
        .into_iter()
        .filter_map(|draft| {
            let task = draft.task.trim().to_owned();
            (!task.is_empty()).then(|| ActionDraft { task, owner: clean(draft.owner), due: clean(draft.due) })
        })
        .collect()
}

/// The message sent to a webhook: `text` and `content` are what Slack and Discord hooks show,
/// `items` is for tools that read JSON (Zapier, n8n, Notion and Jira automations).
pub fn webhook_payload(meeting_title: &str, items: &[ActionDraft]) -> serde_json::Value {
    let lines: Vec<String> = items
        .iter()
        .map(|item| {
            let mut line = format!("• {}", item.task);
            if let Some(owner) = &item.owner {
                line.push_str(&format!(" — {owner}"));
            }
            if let Some(due) = &item.due {
                line.push_str(&format!(" (tenggat {due})"));
            }
            line
        })
        .collect();
    let text = format!("Action item dari rapat \"{meeting_title}\":\n{}", lines.join("\n"));
    serde_json::json!({ "text": text, "content": text, "meeting": meeting_title, "items": items })
}

/// Sends the items of a meeting to `url`. Nothing is sent for an empty list.
pub fn send_webhook(url: &str, meeting_title: &str, items: &[ActionDraft]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let response = client.post(url).json(&webhook_payload(meeting_title, items)).send().map_err(|e| e.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("webhook menolak permintaan ({})", response.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_array_is_found_inside_chatter_and_fences() {
        let reply = "Ini hasilnya:\n```json\n[{\"task\": \"Kirim laporan\", \"owner\": \"Budi\", \"due\": \"Jumat\"}, \
{\"task\": \"  \", \"owner\": null}, {\"task\": \"Siapkan demo\", \"owner\": \"(belum ditentukan)\", \"due\": null}]\n```";
        assert_eq!(
            parse(reply),
            [
                ActionDraft { task: "Kirim laporan".into(), owner: Some("Budi".into()), due: Some("Jumat".into()) },
                ActionDraft { task: "Siapkan demo".into(), owner: None, due: None },
            ]
        );
    }

    #[test]
    fn a_reply_without_a_valid_array_gives_nothing() {
        assert!(parse("Tidak ada action item.").is_empty());
        assert!(parse("[{\"owner\": \"x\"}]").is_empty());
        assert!(parse("[]").is_empty());
    }

    #[test]
    fn the_webhook_payload_reads_as_text_and_as_data() {
        let items = [ActionDraft { task: "Kirim laporan".into(), owner: Some("Budi".into()), due: Some("Jumat".into()) }];
        let payload = webhook_payload("Rapat mingguan", &items);
        assert_eq!(payload["text"], "Action item dari rapat \"Rapat mingguan\":\n• Kirim laporan — Budi (tenggat Jumat)");
        assert_eq!(payload["items"][0]["task"], "Kirim laporan");
    }
}
