//! Questions about all saved meetings at once. The meetings are searched by
//! the words of the question; the minutes of the best matches and the
//! matching transcript lines go to the LLM, which answers from them.

use crate::export::format_date;
use crate::llm::Prompt;
use crate::store::{ChunkHit, Hit, MeetingSummary, Store, StoreError};

/// How many meetings contribute their minutes, how long each may be, and how
/// many transcript lines are quoted.
const MAX_MEETINGS: usize = 6;
const MAX_MOM_CHARS: usize = 1500;
const MAX_QUOTES: usize = 25;
const MAX_SOURCES: usize = 5;
/// The most recent meetings looked at when picking minutes.
const RECENT_LOOKBACK: usize = 50;
const MAX_TERMS: usize = 8;

const SYSTEM: &str = "\
Anda membantu pengguna mencari informasi dari kumpulan rapat yang pernah direkamnya. Jawab dalam \
bahasa Indonesia, singkat dan langsung, hanya berdasarkan notulen dan kutipan transkrip yang \
diberikan. Sebut judul dan tanggal rapat untuk setiap fakta. Jika jawabannya tidak ada di sana, \
katakan bahwa hal itu tidak ditemukan di rapat yang tersimpan. Transkrip berasal dari pengenalan \
suara dan bisa salah dengar. Tulis teks biasa tanpa format Markdown (tanpa tanda * atau #); \
untuk daftar, awali tiap butir dengan \"- \".";

/// Words too common to find anything with.
const STOP_WORDS: &[&str] = &[
    "yang", "dan", "apa", "siapa", "kapan", "dengan", "untuk", "dari", "itu", "ini", "ada", "saja", "tentang", "rapat",
    "semua", "pada", "dalam", "adalah", "akan", "bagaimana", "mengapa", "kenapa", "sudah", "belum", "oleh", "atau",
    "para", "agar", "saya", "kami", "kita", "mereka", "bisa", "dapat", "the", "and",
];

/// A meeting the answer drew on.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Source {
    pub meeting_id: i64,
    pub title: String,
    pub started_at: i64,
}

pub struct Question {
    pub prompt: Prompt,
    /// Meetings with transcript lines that matched, best first.
    pub sources: Vec<Source>,
}

/// Gathers what the LLM needs to answer `question`. `None` while no meeting has
/// minutes or matching transcript. `semantic` are passages found by meaning, which
/// come before the lines found by words. `utc_offset_secs` is the local time zone, for the dates.
pub fn prepare(store: &Store, question: &str, utc_offset_secs: i64, semantic: &[ChunkHit]) -> Result<Option<Question>, StoreError> {
    let hits = store.find_segments(&search_terms(question), MAX_QUOTES)?;
    let recent = store.recent_moms(RECENT_LOOKBACK)?;

    // Minutes of the meetings that matched first, then of the latest ones.
    let matched = semantic.iter().map(|chunk| chunk.meeting.id).chain(hits.iter().map(|hit| hit.meeting.id));
    let mut chosen: Vec<&(MeetingSummary, String)> = Vec::new();
    for meeting_id in matched {
        if let Some(found) = recent.iter().find(|(meeting, _)| meeting.id == meeting_id) {
            if !chosen.iter().any(|(meeting, _)| meeting.id == found.0.id) {
                chosen.push(found);
            }
        }
    }
    for entry in &recent {
        if !chosen.iter().any(|(meeting, _)| meeting.id == entry.0.id) {
            chosen.push(entry);
        }
    }
    chosen.truncate(MAX_MEETINGS);
    if chosen.is_empty() && hits.is_empty() && semantic.is_empty() {
        return Ok(None);
    }

    let minutes: String = chosen
        .iter()
        .map(|(meeting, mom)| format!("### {} ({})\n{}", meeting.title, format_date(meeting.started_at, utc_offset_secs), truncated(mom)))
        .collect::<Vec<_>>()
        .join("\n\n");
    let quote = |meeting: &MeetingSummary, start_secs: f64, speaker: &str, text: &str| {
        let whole = start_secs.max(0.0) as u64;
        format!(
            "[{} · {} · {:02}:{:02}] {}{}",
            meeting.title,
            format_date(meeting.started_at, utc_offset_secs),
            whole / 60,
            whole % 60,
            speaker,
            text
        )
    };
    let quotes: String = semantic
        .iter()
        .map(|chunk| quote(&chunk.meeting, chunk.start_secs, "", &chunk.text))
        .chain(hits.iter().map(|hit| quote(&hit.meeting, hit.segment.start_secs, &format!("{}: ", hit.speaker), &hit.segment.text)))
        .collect::<Vec<_>>()
        .join("\n");

    let user = format!(
        "Notulen rapat-rapat:\n{}\n\nKutipan transkrip yang cocok dengan pertanyaan:\n{}\n\nPertanyaan: {}",
        or_none(&minutes),
        or_none(&quotes),
        question.trim()
    );
    Ok(Some(Question { prompt: Prompt { system: SYSTEM.into(), user }, sources: sources(semantic, &hits) }))
}

/// The distinct meetings of the hits, in the order of the best hit of each.
fn sources(semantic: &[ChunkHit], hits: &[Hit]) -> Vec<Source> {
    let mut sources: Vec<Source> = Vec::new();
    for meeting in semantic.iter().map(|chunk| &chunk.meeting).chain(hits.iter().map(|hit| &hit.meeting)) {
        if sources.len() < MAX_SOURCES && !sources.iter().any(|source| source.meeting_id == meeting.id) {
            sources.push(Source { meeting_id: meeting.id, title: meeting.title.clone(), started_at: meeting.started_at });
        }
    }
    sources
}

/// The distinct lowercase words of the question that are long enough to mean something.
pub fn search_terms(question: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for word in question.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.chars().count() >= 3 && !STOP_WORDS.contains(&word) && !terms.iter().any(|term| term == word) {
            terms.push(word.to_owned());
        }
    }
    terms.truncate(MAX_TERMS);
    terms
}

fn truncated(mom: &str) -> String {
    if mom.chars().count() <= MAX_MOM_CHARS {
        return mom.trim().to_owned();
    }
    let cut: String = mom.chars().take(MAX_MOM_CHARS).collect();
    format!("{}…", cut.trim_end())
}

fn or_none(text: &str) -> &str {
    if text.trim().is_empty() {
        "(belum ada)"
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::NotesSnapshot;
    use crate::transcript::{Source as Origin, TranscriptSegment};

    fn saved(store: &mut Store, title: &str, started_at: i64, mom: &str, text: &str) -> i64 {
        let id = store.create_meeting(title, started_at).unwrap();
        let snapshot = NotesSnapshot {
            segments: vec![TranscriptSegment {
                source: Origin::System,
                start_secs: 61.0,
                end_secs: 65.0,
                text: text.into(),
                speaker: Some(1),
            }],
            mom: mom.into(),
            ..NotesSnapshot::default()
        };
        store.save(id, &snapshot).unwrap();
        id
    }

    fn store() -> Store {
        Store::prepare(rusqlite::Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn terms_drop_short_common_and_repeated_words() {
        assert_eq!(
            search_terms("Apa tenggat anggaran, dan tenggat vendor di rapat?"),
            ["tenggat", "anggaran", "vendor"]
        );
        assert!(search_terms("apa itu?").is_empty());
    }

    #[test]
    fn nothing_saved_means_nothing_to_ask() {
        assert!(prepare(&store(), "apa keputusan anggaran?", 0, &[]).unwrap().is_none());
    }

    #[test]
    fn the_prompt_quotes_matching_lines_and_names_their_meeting() {
        let mut store = store();
        let budget = saved(&mut store, "Rapat anggaran", 1_709_249_400, "## Ringkasan\n- Anggaran", "Anggaran disetujui Jumat.");
        saved(&mut store, "Mingguan", 1_709_000_000, "## Ringkasan\n- Status", "Semua lancar.");

        let question = prepare(&store, "Kapan anggaran disetujui?", 0, &[]).unwrap().unwrap();
        assert!(question.prompt.user.contains("[Rapat anggaran · 2024-02-29 23:30 · 01:01] Pembicara 1: Anggaran disetujui Jumat."));
        assert!(question.prompt.user.contains("### Mingguan"), "minutes of other meetings give context too");
        assert!(question.prompt.user.ends_with("Pertanyaan: Kapan anggaran disetujui?"));
        assert_eq!(
            question.sources,
            [Source { meeting_id: budget, title: "Rapat anggaran".into(), started_at: 1_709_249_400 }]
        );
    }

    #[test]
    fn a_question_without_matches_still_gets_the_latest_minutes() {
        let mut store = store();
        saved(&mut store, "Mingguan", 1, "## Ringkasan\n- Status", "Semua lancar.");
        let question = prepare(&store, "Apa action item terakhir?", 0, &[]).unwrap().unwrap();
        assert!(question.prompt.user.contains("- Status"));
        assert!(question.sources.is_empty());
    }

    #[test]
    fn passages_found_by_meaning_come_first_even_when_no_word_matches() {
        let mut store = store();
        let costs = saved(&mut store, "Rapat dana", 1_709_249_400, "## Ringkasan
- Dana", "Server makin mahal.");
        let meeting = MeetingSummary { id: costs, title: "Rapat dana".into(), started_at: 1_709_249_400, duration_secs: 0.0 };
        let passage = ChunkHit { meeting, start_secs: 5.0, text: "Biaya server naik tiga puluh persen.".into(), score: 0.8 };

        let question = prepare(&store, "Mengapa pengeluaran membengkak?", 0, &[passage]).unwrap().unwrap();

        assert!(question.prompt.user.contains("[Rapat dana · 2024-02-29 23:30 · 00:05] Biaya server naik tiga puluh persen."));
        assert_eq!(question.sources.len(), 1);
        assert_eq!(question.sources[0].meeting_id, costs);
    }

    #[test]
    fn long_minutes_are_cut() {
        assert_eq!(truncated(&"a".repeat(2000)).chars().count(), MAX_MOM_CHARS + 1);
        assert_eq!(truncated("  pendek  "), "pendek");
    }
}
