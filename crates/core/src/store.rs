//! Meetings on disk, in one SQLite file.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::embed::{cosine, from_bytes, to_bytes};
use crate::notes::{Bookmark, Exchange, NotesSnapshot};
use crate::prompts::MomTemplate;
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
);
CREATE TABLE IF NOT EXISTS bookmarks (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    at_secs    REAL NOT NULL,
    label      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chunks (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    start_secs REAL NOT NULL,
    text       TEXT NOT NULL,
    model      TEXT NOT NULL,
    vector     BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS chunks_by_meeting ON chunks(meeting_id);";

/// Columns added after the first release, for databases created before them.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[("meetings", "template", "TEXT NOT NULL DEFAULT 'general'")];

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

    pub(crate) fn prepare(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.execute_batch(SCHEMA)?;
        for (table, column, definition) in ADDED_COLUMNS {
            let present: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"),
                [],
                |row| row.get(0),
            )?;
            if present == 0 {
                connection.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
            }
        }
        Ok(Self(connection))
    }

    /// Removes the meeting with everything stored for it. Returns whether it existed.
    pub fn delete(&self, meeting_id: i64) -> Result<bool, StoreError> {
        Ok(self.0.execute("DELETE FROM meetings WHERE id = ?1", [meeting_id])? > 0)
    }

    pub fn create_meeting(&self, title: &str, started_at: i64) -> Result<i64, StoreError> {
        self.0
            .execute("INSERT INTO meetings (title, started_at) VALUES (?1, ?2)", params![title, started_at])?;
        Ok(self.0.last_insert_rowid())
    }

    /// Changes the title of a meeting. Returns whether it existed.
    pub fn rename(&self, meeting_id: i64, title: &str) -> Result<bool, StoreError> {
        Ok(self.0.execute("UPDATE meetings SET title = ?2 WHERE id = ?1", params![meeting_id, title])? > 0)
    }

    /// Replaces what is stored for the meeting with the snapshot.
    pub fn save(&mut self, meeting_id: i64, snapshot: &NotesSnapshot) -> Result<(), StoreError> {
        let duration_secs = snapshot.segments.iter().map(|segment| segment.end_secs).fold(0.0, f64::max);
        let transaction = self.0.transaction()?;
        transaction.execute(
            "UPDATE meetings SET duration_secs = ?2, mom = ?3, user_notes = ?4, template = ?5 WHERE id = ?1",
            params![meeting_id, duration_secs, snapshot.mom, snapshot.user_notes, snapshot.template.id()],
        )?;
        for table in ["segments", "speaker_names", "chat", "bookmarks"] {
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
            for bookmark in &snapshot.bookmarks {
                transaction.execute(
                    "INSERT INTO bookmarks (meeting_id, at_secs, label) VALUES (?1, ?2, ?3)",
                    params![meeting_id, bookmark.at_secs, bookmark.label],
                )?;
            }
        }
        Ok(transaction.commit()?)
    }

    /// Newest first. A non-blank `query` keeps the meetings whose title or transcript contains it.
    pub fn list(&self, query: &str) -> Result<Vec<MeetingSummary>, StoreError> {
        let pattern = like_pattern(query.trim());
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

    /// Newest first, only meetings that have minutes.
    pub fn recent_moms(&self, limit: usize) -> Result<Vec<(MeetingSummary, String)>, StoreError> {
        let mut select = self.0.prepare(
            "SELECT id, title, started_at, duration_secs, mom FROM meetings
             WHERE mom <> '' ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        let moms = select
            .query_map([limit as i64], |row| Ok((summary_from_row(row)?, row.get::<_, String>(4)?)))?
            .collect::<Result<_, _>>()?;
        Ok(moms)
    }

    /// The transcript segments, across all meetings, that contain the most of
    /// `terms` (lowercase), best first and newest first among equals.
    pub fn find_segments(&self, terms: &[String], limit: usize) -> Result<Vec<Hit>, StoreError> {
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let patterns: Vec<String> = terms.iter().map(|term| like_pattern(term)).collect();
        let condition =
            (1..=patterns.len()).map(|n| format!("s.text LIKE ?{n} ESCAPE '\\'")).collect::<Vec<_>>().join(" OR ");
        let mut select = self.0.prepare(&format!(
            "SELECT m.id, m.title, m.started_at, m.duration_secs, s.source, s.start_secs, s.end_secs, s.text, s.speaker
             FROM segments s JOIN meetings m ON m.id = s.meeting_id WHERE {condition}"
        ))?;
        let mut found: Vec<(usize, Hit)> = select
            .query_map(rusqlite::params_from_iter(&patterns), |row| {
                let segment = TranscriptSegment {
                    source: source_from_name(&row.get::<_, String>(4)?),
                    start_secs: row.get(5)?,
                    end_secs: row.get(6)?,
                    text: row.get(7)?,
                    speaker: row.get(8)?,
                };
                Ok((summary_from_row(row)?, segment))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(meeting, segment)| {
                let text = segment.text.to_lowercase();
                let score = terms.iter().filter(|term| text.contains(term.as_str())).count();
                (score, Hit { meeting, segment, speaker: String::new() })
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.meeting.started_at.cmp(&a.1.meeting.started_at)));
        found.truncate(limit);

        let mut names: HashMap<i64, SpeakerNames> = HashMap::new();
        for (_, hit) in &mut found {
            let meeting_names = match names.entry(hit.meeting.id) {
                Entry::Occupied(known) => known.into_mut(),
                Entry::Vacant(slot) => slot.insert(
                    self.0
                        .prepare("SELECT speaker, name FROM speaker_names WHERE meeting_id = ?1")?
                        .query_map([hit.meeting.id], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .collect::<Result<SpeakerNames, _>>()?,
                ),
            };
            hit.speaker = meeting_names.label(&hit.segment);
        }
        Ok(found.into_iter().map(|(_, hit)| hit).collect())
    }

    /// Meetings with a transcript that have no embedded passages from `model` yet.
    pub fn meetings_to_index(&self, model: &str) -> Result<Vec<i64>, StoreError> {
        let mut select = self.0.prepare(
            "SELECT id FROM meetings m
             WHERE EXISTS (SELECT 1 FROM segments WHERE meeting_id = m.id)
               AND NOT EXISTS (SELECT 1 FROM chunks WHERE meeting_id = m.id AND model = ?1)
             ORDER BY started_at DESC",
        )?;
        let ids = select.query_map([model], |row| row.get(0))?.collect::<Result<_, _>>()?;
        Ok(ids)
    }

    /// Replaces the embedded passages of the meeting.
    pub fn replace_chunks(&mut self, meeting_id: i64, model: &str, chunks: &[(f64, String, Vec<f32>)]) -> Result<(), StoreError> {
        let transaction = self.0.transaction()?;
        transaction.execute("DELETE FROM chunks WHERE meeting_id = ?1", [meeting_id])?;
        {
            let mut insert = transaction
                .prepare("INSERT INTO chunks (meeting_id, start_secs, text, model, vector) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for (start_secs, text, vector) in chunks {
                insert.execute(params![meeting_id, start_secs, text, model, to_bytes(vector)])?;
            }
        }
        Ok(transaction.commit()?)
    }

    /// The passages of all meetings closest in meaning to `query`, best first. Only
    /// passages embedded by `model` count, and only those scoring at least `min_score`.
    pub fn nearest_chunks(&self, model: &str, query: &[f32], min_score: f32, limit: usize) -> Result<Vec<ChunkHit>, StoreError> {
        let mut select = self.0.prepare(
            "SELECT m.id, m.title, m.started_at, m.duration_secs, c.start_secs, c.text, c.vector
             FROM chunks c JOIN meetings m ON m.id = c.meeting_id WHERE c.model = ?1",
        )?;
        let mut found: Vec<ChunkHit> = Vec::new();
        let mut rows = select.query([model])?;
        while let Some(row) = rows.next()? {
            let score = cosine(query, &from_bytes(&row.get::<_, Vec<u8>>(6)?));
            if score >= min_score {
                found.push(ChunkHit { meeting: summary_from_row(row)?, start_secs: row.get(4)?, text: row.get(5)?, score });
            }
        }
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        found.truncate(limit);
        Ok(found)
    }

    pub fn load(&self, meeting_id: i64) -> Result<Option<(MeetingSummary, NotesSnapshot)>, StoreError> {
        let meeting = self
            .0
            .query_row(
                "SELECT id, title, started_at, duration_secs, mom, user_notes, template FROM meetings WHERE id = ?1",
                [meeting_id],
                |row| {
                    Ok((summary_from_row(row)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?))
                },
            )
            .optional()?;
        let Some((summary, mom, user_notes, template)) = meeting else {
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

        let bookmarks = self
            .0
            .prepare("SELECT at_secs, label FROM bookmarks WHERE meeting_id = ?1 ORDER BY at_secs")?
            .query_map([meeting_id], |row| Ok(Bookmark { at_secs: row.get(0)?, label: row.get(1)? }))?
            .collect::<Result<_, _>>()?;
        let template = MomTemplate::from_id(&template).unwrap_or_default();

        Ok(Some((summary, NotesSnapshot { segments, mom, user_notes, names, chat, template, bookmarks })))
    }
}

/// A passage of a transcript found by meaning, with the meeting it is from.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub meeting: MeetingSummary,
    pub start_secs: f64,
    pub text: String,
    pub score: f32,
}

/// One transcript segment found by `Store::find_segments`, with the meeting it is from.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub meeting: MeetingSummary,
    pub segment: TranscriptSegment,
    /// The label shown for whoever spoke, with the names given in that meeting.
    pub speaker: String,
}

/// A LIKE pattern that matches `text` anywhere, with wildcards in `text` taken literally.
fn like_pattern(text: &str) -> String {
    format!("%{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
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
            ..NotesSnapshot::default()
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
    fn a_meeting_can_be_renamed() {
        let store = store();
        let id = store.create_meeting("Rapat tanpa judul", 1).unwrap();
        assert!(store.rename(id, "Anggaran").unwrap());
        assert_eq!(store.load(id).unwrap().unwrap().0.title, "Anggaran");
        assert!(!store.rename(99, "x").unwrap());
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
    fn template_and_bookmarks_are_saved_and_a_meeting_can_be_deleted() {
        let mut store = store();
        let id = store.create_meeting("Rapat", 1).unwrap();
        let mut with_extras = snapshot();
        with_extras.template = MomTemplate::Sales;
        with_extras.bookmarks = vec![Bookmark { at_secs: 5.0, label: "harga".into() }];
        store.save(id, &with_extras).unwrap();
        assert_eq!(store.load(id).unwrap().unwrap().1, with_extras);

        assert!(store.delete(id).unwrap());
        assert!(store.load(id).unwrap().is_none());
        assert!(store.find_segments(&["anggaran".into()], 10).unwrap().is_empty());
        assert!(!store.delete(id).unwrap());
    }

    #[test]
    fn found_segments_are_ranked_by_matching_terms_and_carry_speaker_names() {
        let mut store = store();
        let old = store.create_meeting("Lama", 100).unwrap();
        let new = store.create_meeting("Baru", 200).unwrap();
        let mut both = snapshot();
        both.segments.push(segment(Source::System, 20.0, "anggaran dan tenggat Jumat", Some(1)));
        store.save(old, &both).unwrap();
        store.save(new, &snapshot()).unwrap();

        let hits = store.find_segments(&["anggaran".into(), "tenggat".into()], 10).unwrap();
        assert_eq!(hits[0].segment.text, "anggaran dan tenggat Jumat");
        assert_eq!(hits[0].speaker, "Budi");
        // Equal scores: the newer meeting first.
        assert_eq!((hits[1].meeting.id, hits[2].meeting.id), (new, old));
        assert_eq!(store.find_segments(&["anggaran".into()], 1).unwrap().len(), 1);
        assert!(store.find_segments(&[], 10).unwrap().is_empty());
    }

    #[test]
    fn recent_minutes_skip_meetings_without_any() {
        let mut store = store();
        store.create_meeting("Kosong", 300).unwrap();
        let written = store.create_meeting("Ada", 100).unwrap();
        store.save(written, &snapshot()).unwrap();
        let moms = store.recent_moms(5).unwrap();
        assert_eq!(moms.len(), 1);
        assert_eq!(moms[0].0.id, written);
    }

    #[test]
    fn search_treats_wildcards_as_plain_text() {
        let store = store();
        store.create_meeting("Rapat", 1).unwrap();
        assert!(store.list("%").unwrap().is_empty());
    }
}
