//! Meetings on disk, in one SQLite file.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::notes::{Exchange, NotesSnapshot};
use crate::speakers::SpeakerNames;
use crate::transcript::{Source, TranscriptSegment};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meetings (
    id            INTEGER PRIMARY KEY,
    title         TEXT NOT NULL,
    started_at    INTEGER NOT NULL,
    duration_secs REAL NOT NULL DEFAULT 0,
    mom           TEXT NOT NULL DEFAULT '',
    user_notes    TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS segments (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source     TEXT NOT NULL,
    start_secs REAL NOT NULL,
    end_secs   REAL NOT NULL,
    text       TEXT NOT NULL,
    speaker    INTEGER
);
CREATE INDEX IF NOT EXISTS segments_by_meeting ON segments(meeting_id, start_secs);
CREATE TABLE IF NOT EXISTS speaker_names (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    speaker    INTEGER NOT NULL,
    name       TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chat (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    question   TEXT NOT NULL,
    answer     TEXT NOT NULL
);";

#[derive(Debug, thiserror::Error)]
#[error("storage failed: {0}")]
pub struct StoreError(#[from] rusqlite::Error);

/// One row of the meeting list.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeetingSummary {
    pub id: i64,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub started_at: i64,
    pub duration_secs: f64,
}

pub struct Store(Connection);

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::prepare(Connection::open(path)?)
    }

    fn prepare(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self(connection))
    }

    pub fn create_meeting(&self, title: &str, started_at: i64) -> Result<i64, StoreError> {
        self.0
            .execute("INSERT INTO meetings (title, started_at) VALUES (?1, ?2)", params![title, started_at])?;
        Ok(self.0.last_insert_rowid())
    }

    /// Replaces what is stored for the meeting with the snapshot.
    pub fn save(&mut self, meeting_id: i64, snapshot: &NotesSnapshot) -> Result<(), StoreError> {
        let duration_secs = snapshot.segments.iter().map(|segment| segment.end_secs).fold(0.0, f64::max);
        let transaction = self.0.transaction()?;
        transaction.execute(
            "UPDATE meetings SET duration_secs = ?2, mom = ?3, user_notes = ?4 WHERE id = ?1",
            params![meeting_id, duration_secs, snapshot.mom, snapshot.user_notes],
        )?;
        for table in ["segments", "speaker_names", "chat"] {
            transaction.execute(&format!("DELETE FROM {table} WHERE meeting_id = ?1"), [meeting_id])?;
        }
        {
            let mut insert = transaction.prepare(
                "INSERT INTO segments (meeting_id, source, start_secs, end_secs, text, speaker)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for segment in &snapshot.segments {
                insert.execute(params![
                    meeting_id,
                    source_name(segment.source),
                    segment.start_secs,
                    segment.end_secs,
                    segment.text,
                    segment.speaker
                ])?;
            }
            for (speaker, name) in snapshot.names.given() {
                transaction.execute(
                    "INSERT INTO speaker_names (meeting_id, speaker, name) VALUES (?1, ?2, ?3)",
                    params![meeting_id, speaker, name],
                )?;
            }
            for (position, exchange) in snapshot.chat.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO chat (meeting_id, position, question, answer) VALUES (?1, ?2, ?3, ?4)",
                    params![meeting_id, position as i64, exchange.question, exchange.answer],
                )?;
            }
        }
        Ok(transaction.commit()?)
    }

    /// Newest first. A non-blank `query` keeps the meetings whose title or transcript contains it.
    pub fn list(&self, query: &str) -> Result<Vec<MeetingSummary>, StoreError> {
        let pattern = format!("%{}%", query.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let mut select = self.0.prepare(
            "SELECT id, title, started_at, duration_secs FROM meetings
             WHERE title LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM segments
                           WHERE meeting_id = meetings.id AND text LIKE ?1 ESCAPE '\\')
             ORDER BY started_at DESC, id DESC",
        )?;
        let summaries = select.query_map([pattern], summary_from_row)?.collect::<Result<_, _>>()?;
        Ok(summaries)
    }

    pub fn load(&self, meeting_id: i64) -> Result<Option<(MeetingSummary, NotesSnapshot)>, StoreError> {
        let meeting = self
            .0
            .query_row(
                "SELECT id, title, started_at, duration_secs, mom, user_notes FROM meetings WHERE id = ?1",
                [meeting_id],
                |row| Ok((summary_from_row(row)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?)),
            )
            .optional()?;
        let Some((summary, mom, user_notes)) = meeting else {
            return Ok(None);
        };

        let segments = self
            .0
            .prepare(
                "SELECT source, start_secs, end_secs, text, speaker FROM segments
                 WHERE meeting_id = ?1 ORDER BY start_secs",
            )?
            .query_map([meeting_id], |row| {
                Ok(TranscriptSegment {
                    source: source_from_name(&row.get::<_, String>(0)?),
                    start_secs: row.get(1)?,
                    end_secs: row.get(2)?,
                    text: row.get(3)?,
                    speaker: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let names = self
            .0
            .prepare("SELECT speaker, name FROM speaker_names WHERE meeting_id = ?1")?
            .query_map([meeting_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<SpeakerNames, _>>()?;
        let chat = self
            .0
            .prepare("SELECT question, answer FROM chat WHERE meeting_id = ?1 ORDER BY position")?
            .query_map([meeting_id], |row| Ok(Exchange { question: row.get(0)?, answer: row.get(1)? }))?
            .collect::<Result<_, _>>()?;

        Ok(Some((summary, NotesSnapshot { segments, mom, user_notes, names, chat })))
    }
}

fn summary_from_row(row: &rusqlite::Row) -> rusqlite::Result<MeetingSummary> {
    Ok(MeetingSummary { id: row.get(0)?, title: row.get(1)?, started_at: row.get(2)?, duration_secs: row.get(3)? })
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Microphone => "microphone",
        Source::System => "system",
    }
}

fn source_from_name(name: &str) -> Source {
    match name {
        "microphone" => Source::Microphone,
        _ => Source::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn segment(source: Source, start_secs: f64, text: &str, speaker: Option<u32>) -> TranscriptSegment {
        TranscriptSegment { source, start_secs, end_secs: start_secs + 4.0, text: text.into(), speaker }
    }

    fn snapshot() -> NotesSnapshot {
        NotesSnapshot {
            segments: vec![
                segment(Source::Microphone, 0.0, "Selamat pagi.", None),
                segment(Source::System, 5.0, "Anggaran disetujui.", Some(1)),
            ],
            mom: "## Ringkasan\n- Anggaran".into(),
            user_notes: "fokus anggaran".into(),
            names: [(1, "Budi".to_string())].into_iter().collect(),
            chat: vec![Exchange { question: "Siapa?".into(), answer: "Budi.".into() }],
        }
    }

    #[test]
    fn a_saved_meeting_is_loaded_back_unchanged() {
        let mut store = store();
        let id = store.create_meeting("Rapat anggaran", 1_800_000_000).unwrap();
        store.save(id, &snapshot()).unwrap();

        let (summary, loaded) = store.load(id).unwrap().unwrap();
        assert_eq!(
            summary,
            MeetingSummary { id, title: "Rapat anggaran".into(), started_at: 1_800_000_000, duration_secs: 9.0 }
        );
        assert_eq!(loaded, snapshot());
    }

    #[test]
    fn saving_again_replaces_the_previous_content() {
        let mut store = store();
        let id = store.create_meeting("Rapat", 1).unwrap();
        store.save(id, &snapshot()).unwrap();
        let mut shorter = snapshot();
        shorter.segments.truncate(1);
        shorter.chat.clear();
        store.save(id, &shorter).unwrap();
        assert_eq!(store.load(id).unwrap().unwrap().1, shorter);
    }

    #[test]
    fn loading_an_unknown_meeting_yields_nothing() {
        assert!(store().load(42).unwrap().is_none());
    }

    #[test]
    fn meetings_are_listed_newest_first() {
        let store = store();
        let older = store.create_meeting("Lama", 100).unwrap();
        let newer = store.create_meeting("Baru", 200).unwrap();
        let ids: Vec<i64> = store.list("").unwrap().iter().map(|meeting| meeting.id).collect();
        assert_eq!(ids, [newer, older]);
    }

    #[test]
    fn search_matches_title_or_transcript_and_ignores_case() {
        let mut store = store();
        let by_title = store.create_meeting("Rapat Anggaran", 1).unwrap();
        let by_text = store.create_meeting("Mingguan", 2).unwrap();
        let other = store.create_meeting("Lainnya", 3).unwrap();
        store.save(by_text, &snapshot()).unwrap();

        let found: Vec<i64> = store.list(" anggaran ").unwrap().iter().map(|meeting| meeting.id).collect();
        assert_eq!(found, [by_text, by_title]);
        assert!(!found.contains(&other));
    }

    #[test]
    fn search_treats_wildcards_as_plain_text() {
        let store = store();
        store.create_meeting("Rapat", 1).unwrap();
        assert!(store.list("%").unwrap().is_empty());
    }
}
