//! What is known about one meeting (transcript, minutes, the user's own
//! notes) and the LLM calls that derive minutes and answers from it.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::llm::{LlmClient, LlmError};
use crate::prompts;
use crate::speakers::{assign_speakers, replace_label, SpeakerNames, SpeakerTurn};
use crate::transcript::TranscriptSegment;

pub struct MeetingNotes {
    llm: Arc<dyn LlmClient>,
    state: Mutex<State>,
}

/// Everything worth keeping of one meeting; what is saved and loaded.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct NotesSnapshot {
    /// In chronological order.
    pub segments: Vec<TranscriptSegment>,
    pub mom: String,
    pub user_notes: String,
    pub names: SpeakerNames,
    pub chat: Vec<Exchange>,
}

/// One question asked about the meeting and its answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Exchange {
    pub question: String,
    pub answer: String,
}

#[derive(Default)]
struct State {
    chat: Vec<Exchange>,
    /// In arrival order, which is not always chronological across the two streams.
    segments: Vec<TranscriptSegment>,
    /// How many of `segments` the current minutes already cover.
    summarized: usize,
    mom: String,
    user_notes: String,
    names: SpeakerNames,
}

impl MeetingNotes {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm, state: Mutex::default() }
    }

    /// Continues a saved meeting. Its minutes are taken to cover the whole transcript.
    pub fn restore(llm: Arc<dyn LlmClient>, snapshot: NotesSnapshot) -> Self {
        let state = State {
            summarized: snapshot.segments.len(),
            segments: snapshot.segments,
            mom: snapshot.mom,
            user_notes: snapshot.user_notes,
            names: snapshot.names,
            chat: snapshot.chat,
        };
        Self { llm, state: Mutex::new(state) }
    }

    pub fn snapshot(&self) -> NotesSnapshot {
        let state = self.state();
        NotesSnapshot {
            segments: chronological(&state.segments),
            mom: state.mom.clone(),
            user_notes: state.user_notes.clone(),
            names: state.names.clone(),
            chat: state.chat.clone(),
        }
    }

    pub fn add_segment(&self, segment: TranscriptSegment) {
        self.state().segments.push(segment);
    }

    pub fn set_user_notes(&self, notes: String) {
        self.state().user_notes = notes;
    }

    /// The transcript so far, in chronological order.
    pub fn transcript(&self) -> Vec<TranscriptSegment> {
        chronological(&self.state().segments)
    }

    /// Labels the system-audio segments with the speakers found by diarization.
    pub fn apply_speaker_turns(&self, turns: &[SpeakerTurn]) {
        assign_speakers(&mut self.state().segments, turns);
    }

    /// Gives a speaker a name, in the transcript sent to the LLM from now on
    /// and in the minutes already written. Returns the minutes.
    pub fn rename_speaker(&self, speaker: u32, name: &str) -> String {
        let mut state = self.state();
        let old = state.names.name(speaker);
        state.names.rename(speaker, name);
        let new = state.names.name(speaker);
        state.mom = replace_label(&state.mom, &old, &new);
        state.mom.clone()
    }

    /// Brings the minutes up to date with the transcript added since the last
    /// successful update. Returns `None` when there is nothing new. After a
    /// failure the same segments are sent again by the next call.
    pub fn update_mom(&self) -> Result<Option<String>, LlmError> {
        let (prompt, covered) = {
            let state = self.state();
            if state.summarized == state.segments.len() {
                return Ok(None);
            }
            let new_segments = chronological(&state.segments[state.summarized..]);
            (
                prompts::incremental_mom(&state.mom, &state.user_notes, &new_segments, &state.names),
                state.segments.len(),
            )
        };
        Ok(Some(self.store_mom(self.llm.complete(&prompt)?, covered)))
    }

    /// Rewrites the minutes from the whole transcript. Returns `None` for a meeting without speech.
    pub fn rebuild_mom(&self) -> Result<Option<String>, LlmError> {
        let (prompt, covered) = {
            let state = self.state();
            if state.segments.is_empty() {
                return Ok(None);
            }
            (
                prompts::final_mom(&state.user_notes, &chronological(&state.segments), &state.names),
                state.segments.len(),
            )
        };
        Ok(Some(self.store_mom(self.llm.complete(&prompt)?, covered)))
    }

    pub fn answer(&self, question: &str) -> Result<String, LlmError> {
        let prompt = {
            let state = self.state();
            prompts::answer(question, &state.mom, &chronological(&state.segments), &state.names)
        };
        let answer = self.llm.complete(&prompt)?;
        self.state().chat.push(Exchange { question: question.trim().to_owned(), answer: answer.clone() });
        Ok(answer)
    }

    fn store_mom(&self, mom: String, covered: usize) -> String {
        let mut state = self.state();
        state.mom.clone_from(&mom);
        state.summarized = covered;
        mom
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The state stays consistent even if a holder panicked: every write is a single assignment.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn chronological(segments: &[TranscriptSegment]) -> Vec<TranscriptSegment> {
    let mut sorted = segments.to_vec();
    sorted.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
    sorted
}

/// Updates the minutes at a fixed interval until stopped.
pub struct PeriodicMom {
    stop: Sender<()>,
    thread: JoinHandle<()>,
}

impl PeriodicMom {
    /// `report` receives every new version of the minutes and every failed attempt.
    pub fn start(
        notes: Arc<MeetingNotes>,
        interval: Duration,
        report: impl Fn(Result<String, LlmError>) + Send + 'static,
    ) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                match notes.update_mom() {
                    Ok(Some(mom)) => report(Ok(mom)),
                    Ok(None) => {}
                    Err(error) => report(Err(error)),
                }
            }
        });
        Self { stop, thread }
    }

    /// Waits for an update that is in flight, then returns.
    pub fn stop(self) {
        drop(self.stop);
        let _ = self.thread.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Prompt;
    use crate::transcript::Source;

    /// Records every prompt and replies with canned results, then with "mom N".
    #[derive(Default)]
    struct FakeLlm {
        prompts: Mutex<Vec<Prompt>>,
        failures_left: Mutex<usize>,
    }

    impl FakeLlm {
        fn failing(times: usize) -> Self {
            Self { failures_left: Mutex::new(times), ..Self::default() }
        }

        fn prompts(&self) -> Vec<Prompt> {
            self.prompts.lock().unwrap().clone()
        }
    }

    impl LlmClient for FakeLlm {
        fn complete(&self, prompt: &Prompt) -> Result<String, LlmError> {
            let mut prompts = self.prompts.lock().unwrap();
            prompts.push(prompt.clone());
            let mut failures_left = self.failures_left.lock().unwrap();
            if *failures_left > 0 {
                *failures_left -= 1;
                return Err(LlmError::Empty);
            }
            Ok(format!("mom {}", prompts.len()))
        }
    }

    fn segment(start_secs: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            source: Source::System,
            start_secs,
            end_secs: start_secs + 1.0,
            text: text.into(),
            speaker: None,
        }
    }

    fn notes_with(llm: &Arc<FakeLlm>) -> MeetingNotes {
        MeetingNotes::new(llm.clone())
    }

    #[test]
    fn update_without_new_transcript_calls_nothing() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        assert_eq!(notes.update_mom().unwrap(), None);
        assert!(llm.prompts().is_empty());
    }

    #[test]
    fn each_update_sends_only_the_transcript_added_since_the_last_one() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);

        notes.add_segment(segment(0.0, "pertama"));
        assert_eq!(notes.update_mom().unwrap().as_deref(), Some("mom 1"));
        notes.add_segment(segment(10.0, "kedua"));
        assert_eq!(notes.update_mom().unwrap().as_deref(), Some("mom 2"));
        assert_eq!(notes.update_mom().unwrap(), None);

        let second = &llm.prompts()[1].user;
        assert!(second.contains("MoM saat ini:\nmom 1"));
        assert!(second.contains("kedua"));
        assert!(!second.contains("pertama"));
    }

    #[test]
    fn segments_of_a_failed_update_are_sent_again() {
        let llm = Arc::new(FakeLlm::failing(1));
        let notes = notes_with(&llm);

        notes.add_segment(segment(0.0, "pertama"));
        assert!(notes.update_mom().is_err());
        notes.add_segment(segment(10.0, "kedua"));
        assert!(notes.update_mom().unwrap().is_some());

        let retry = &llm.prompts()[1].user;
        assert!(retry.contains("pertama") && retry.contains("kedua"));
    }

    #[test]
    fn new_transcript_is_sent_in_chronological_order() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(20.0, "belakangan"));
        notes.add_segment(segment(5.0, "duluan"));
        notes.update_mom().unwrap();

        let user = &llm.prompts()[0].user;
        assert!(user.find("duluan").unwrap() < user.find("belakangan").unwrap());
    }

    #[test]
    fn user_notes_are_passed_to_the_llm() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.set_user_notes("fokus ke anggaran".into());
        notes.add_segment(segment(0.0, "halo"));
        notes.update_mom().unwrap();
        assert!(llm.prompts()[0].user.contains("fokus ke anggaran"));
    }

    #[test]
    fn rebuild_sends_the_whole_transcript_and_marks_it_covered() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(0.0, "pertama"));
        notes.update_mom().unwrap();
        notes.add_segment(segment(10.0, "kedua"));

        assert_eq!(notes.rebuild_mom().unwrap().as_deref(), Some("mom 2"));
        let rebuild = &llm.prompts()[1].user;
        assert!(rebuild.contains("pertama") && rebuild.contains("kedua"));
        assert_eq!(notes.update_mom().unwrap(), None);
    }

    #[test]
    fn rebuild_of_a_meeting_without_speech_calls_nothing() {
        let llm = Arc::new(FakeLlm::default());
        assert_eq!(notes_with(&llm).rebuild_mom().unwrap(), None);
        assert!(llm.prompts().is_empty());
    }

    #[test]
    fn answer_uses_the_whole_transcript_and_current_mom() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(0.0, "tenggat Jumat"));
        notes.update_mom().unwrap();
        notes.answer("Kapan tenggatnya?").unwrap();

        let question = &llm.prompts()[1].user;
        assert!(question.contains("mom 1"));
        assert!(question.contains("tenggat Jumat"));
        assert!(question.ends_with("Pertanyaan: Kapan tenggatnya?"));
    }

    #[test]
    fn speaker_turns_and_names_reach_the_transcript_sent_to_the_llm() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(0.0, "halo"));
        notes.apply_speaker_turns(&[SpeakerTurn { start_secs: 0.0, end_secs: 1.0, speaker: 5 }]);
        assert_eq!(notes.transcript()[0].speaker, Some(1));

        notes.rename_speaker(1, "Budi");
        notes.rebuild_mom().unwrap();
        assert!(llm.prompts()[0].user.contains("[00:00] Budi: halo"));
    }

    #[test]
    fn renaming_a_speaker_rewrites_the_minutes_already_written() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(0.0, "halo"));
        notes.update_mom().unwrap();
        notes.state().mom = "- [Pembicara 1] kirim laporan".into();

        assert_eq!(notes.rename_speaker(1, "Budi"), "- [Budi] kirim laporan");
        assert_eq!(notes.rename_speaker(1, "Budi Santoso"), "- [Budi Santoso] kirim laporan");
        assert_eq!(notes.rename_speaker(1, ""), "- [Pembicara 1] kirim laporan");
    }

    #[test]
    fn a_restored_meeting_keeps_everything_and_needs_no_update() {
        let llm = Arc::new(FakeLlm::default());
        let notes = notes_with(&llm);
        notes.add_segment(segment(0.0, "halo"));
        notes.set_user_notes("catatan".into());
        notes.update_mom().unwrap();
        notes.rename_speaker(1, "Budi");
        notes.answer(" Siapa? ").unwrap();

        let snapshot = notes.snapshot();
        assert_eq!(snapshot.chat, [Exchange { question: "Siapa?".into(), answer: "mom 2".into() }]);

        let restored = MeetingNotes::restore(llm.clone(), snapshot.clone());
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(restored.update_mom().unwrap(), None);
    }

    #[test]
    fn periodic_updates_report_new_minutes_until_stopped() {
        let llm = Arc::new(FakeLlm::default());
        let notes = Arc::new(notes_with(&llm));
        notes.add_segment(segment(0.0, "halo"));

        let (reports, received) = mpsc::channel();
        let periodic = PeriodicMom::start(notes, Duration::from_millis(10), move |result| {
            let _ = reports.send(result.map_err(|error| error.to_string()));
        });
        let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
        periodic.stop();

        assert_eq!(first, Ok("mom 1".to_string()));
        // Nothing new was added, so no further call was made.
        assert_eq!(llm.prompts().len(), 1);
    }
}
