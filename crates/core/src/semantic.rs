//! Finding what meetings said by meaning. Each transcript is cut into passages,
//! which are embedded once after the meeting ends; a question is embedded when
//! it is asked and compared with all passages.

use std::sync::{Mutex, PoisonError};

use crate::embed::{EmbedError, Embedder};
use crate::store::{ChunkHit, Store, StoreError};
use crate::transcript::TranscriptSegment;

/// A passage is cut at the first segment end past this many characters.
const CHUNK_CHARS: usize = 450;
/// Passages less similar than this to the question are not related.
pub const MIN_SCORE: f32 = 0.6;
/// Scores of unrelated and related texts are close together (about 0.60 and 0.68 for Gemini),
/// so what counts is being near the best match, not the score itself.
const MARGIN: f32 = 0.06;

#[derive(Debug, thiserror::Error)]
pub enum SemanticError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Embed(#[from] EmbedError),
}

/// A stretch of a transcript that is embedded as one piece.
#[derive(Debug, PartialEq)]
pub struct Passage {
    pub start_secs: f64,
    pub text: String,
}

/// Consecutive segments grouped into passages of a few sentences each.
pub fn passages(segments: &[TranscriptSegment]) -> Vec<Passage> {
    let mut ordered: Vec<&TranscriptSegment> = segments.iter().filter(|segment| !segment.text.trim().is_empty()).collect();
    ordered.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));

    let mut result: Vec<Passage> = Vec::new();
    let mut current: Option<Passage> = None;
    for segment in ordered {
        let passage = current.get_or_insert_with(|| Passage { start_secs: segment.start_secs, text: String::new() });
        if !passage.text.is_empty() {
            passage.text.push(' ');
        }
        passage.text.push_str(segment.text.trim());
        if passage.text.chars().count() >= CHUNK_CHARS {
            result.extend(current.take());
        }
    }
    result.extend(current);
    result
}

fn locked(db: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    db.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Embeds the transcript of the meeting and stores the passages. Returns how many there are.
/// The store is only locked while reading and writing, not while the provider answers.
pub fn index_meeting(db: &Mutex<Store>, embedder: &dyn Embedder, meeting_id: i64) -> Result<usize, SemanticError> {
    let loaded = locked(db).load(meeting_id)?;
    let Some((_, snapshot)) = loaded else {
        return Ok(0);
    };
    let passages = passages(&snapshot.segments);
    let texts: Vec<String> = passages.iter().map(|passage| passage.text.clone()).collect();
    let vectors = embedder.embed(&texts)?;
    let rows: Vec<(f64, String, Vec<f32>)> =
        passages.into_iter().zip(vectors).map(|(passage, vector)| (passage.start_secs, passage.text, vector)).collect();
    locked(db).replace_chunks(meeting_id, embedder.model(), &rows)?;
    Ok(rows.len())
}

/// Embeds every meeting that has not been embedded with this model yet. Returns how many were done.
/// Stops at the first failure, so a provider without embeddings is asked once, not once per meeting.
pub fn index_pending(db: &Mutex<Store>, embedder: &dyn Embedder) -> Result<usize, SemanticError> {
    let pending = locked(db).meetings_to_index(embedder.model())?;
    for meeting_id in &pending {
        index_meeting(db, embedder, *meeting_id)?;
    }
    Ok(pending.len())
}

/// The passages closest in meaning to `question`, best first.
pub fn search(db: &Mutex<Store>, embedder: &dyn Embedder, question: &str, limit: usize) -> Result<Vec<ChunkHit>, SemanticError> {
    let vectors = embedder.embed(&[question.trim().to_owned()])?;
    let Some(query) = vectors.first() else {
        return Ok(Vec::new());
    };
    let mut found = locked(db).nearest_chunks(embedder.model(), query, MIN_SCORE, limit)?;
    if let Some(best) = found.first().map(|hit| hit.score) {
        found.retain(|hit| hit.score >= best - MARGIN);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::NotesSnapshot;
    use crate::transcript::Source;

    /// Puts texts about money on one axis and texts about people on another.
    struct Topics;

    impl Embedder for Topics {
        fn model(&self) -> &str {
            "topics"
        }

        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
            Ok(texts
                .iter()
                .map(|text| {
                    let text = text.to_lowercase();
                    let has = |words: &[&str]| if words.iter().any(|word| text.contains(word)) { 1.0 } else { 0.0 };
                    vec![has(&["biaya", "anggaran", "dana"]), has(&["tim", "rekrut", "karyawan"]), 0.1]
                })
                .collect())
        }
    }

    fn segment(start_secs: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment { source: Source::System, start_secs, end_secs: start_secs + 3.0, text: text.into(), speaker: None }
    }

    fn meeting(db: &Mutex<Store>, title: &str, started_at: i64, lines: &[&str]) -> i64 {
        let mut store = db.lock().unwrap();
        let id = store.create_meeting(title, started_at).unwrap();
        let segments = lines.iter().enumerate().map(|(i, text)| segment(i as f64 * 10.0, text)).collect();
        store.save(id, &NotesSnapshot { segments, ..NotesSnapshot::default() }).unwrap();
        id
    }

    fn db() -> Mutex<Store> {
        Mutex::new(Store::prepare(rusqlite::Connection::open_in_memory().unwrap()).unwrap())
    }

    #[test]
    fn short_segments_are_grouped_and_long_ones_split_the_passages() {
        let long = "x".repeat(CHUNK_CHARS);
        let found = passages(&[segment(0.0, "satu"), segment(4.0, "dua"), segment(8.0, &long), segment(12.0, "tiga"), segment(16.0, "   ")]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], Passage { start_secs: 0.0, text: format!("satu dua {long}") });
        assert_eq!(found[1], Passage { start_secs: 12.0, text: "tiga".into() });
        assert!(passages(&[]).is_empty());
    }

    #[test]
    fn a_question_finds_the_meeting_that_talked_about_the_same_thing_in_other_words() {
        let db = db();
        let money = meeting(&db, "Rapat dana", 100, &["Biaya server naik bulan ini."]);
        meeting(&db, "Rapat tim", 200, &["Kita perlu rekrut dua karyawan."]);
        assert_eq!(index_pending(&db, &Topics).unwrap(), 2);

        let hits = search(&db, &Topics, "Berapa anggaran kita?", 5).unwrap();

        assert_eq!(hits.len(), 1, "the other meeting is not related");
        assert_eq!(hits[0].meeting.id, money);
        assert_eq!(hits[0].text, "Biaya server naik bulan ini.");
    }

    #[test]
    fn a_meeting_is_only_embedded_once_per_model() {
        let db = db();
        meeting(&db, "Rapat", 1, &["Biaya naik."]);
        assert_eq!(index_pending(&db, &Topics).unwrap(), 1);
        assert_eq!(index_pending(&db, &Topics).unwrap(), 0);
    }

    #[test]
    fn deleting_a_meeting_removes_its_passages() {
        let db = db();
        let id = meeting(&db, "Rapat", 1, &["Biaya naik."]);
        index_pending(&db, &Topics).unwrap();
        db.lock().unwrap().delete(id).unwrap();
        assert!(search(&db, &Topics, "anggaran", 5).unwrap().is_empty());
    }
}
