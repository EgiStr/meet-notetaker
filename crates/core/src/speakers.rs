//! Who said what: the diarization interface, matching its speaker turns to
//! transcript segments, and the names shown for each speaker.

use std::collections::BTreeMap;
use std::path::Path;

use crate::transcript::{Source, TranscriptSegment};

/// A stretch of a recording in which one voice speaks.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerTurn {
    pub start_secs: f64,
    pub end_secs: f64,
    /// Any identifier that is the same for the same voice within one recording.
    pub speaker: u32,
}

#[derive(Debug, thiserror::Error)]
#[error("diarization failed: {0}")]
pub struct DiarizeError(pub String);

pub trait Diarizer: Send + Sync {
    /// `recording` is a 16 kHz mono WAV file that starts when the session started.
    fn diarize(&self, recording: &Path) -> Result<Vec<SpeakerTurn>, DiarizeError>;
}

/// A voice that speaks less than this share of all speech (and less than `MINOR_MAX_SECS`) is
/// almost always noise or the same person split in two, not a participant.
const MINOR_SHARE: f64 = 0.03;
const MINOR_MAX_SECS: f64 = 30.0;

/// Gives the turns of minor voices to the nearest turn in time of a main voice, so a meeting
/// shows a handful of speakers instead of a dozen. Nothing changes if every voice is minor.
pub fn merge_minor_voices(turns: &[SpeakerTurn]) -> Vec<SpeakerTurn> {
    let mut total: BTreeMap<u32, f64> = BTreeMap::new();
    for turn in turns {
        *total.entry(turn.speaker).or_default() += turn.end_secs - turn.start_secs;
    }
    let all: f64 = total.values().sum();
    let is_minor = |voice: u32| {
        let secs = total.get(&voice).copied().unwrap_or(0.0);
        secs < MINOR_MAX_SECS && secs < all * MINOR_SHARE
    };
    let main: Vec<&SpeakerTurn> = turns.iter().filter(|turn| !is_minor(turn.speaker)).collect();
    if main.is_empty() {
        return turns.to_vec();
    }
    turns
        .iter()
        .map(|turn| {
            if !is_minor(turn.speaker) {
                return turn.clone();
            }
            let gap = |other: &&SpeakerTurn| (other.start_secs - turn.end_secs).max(turn.start_secs - other.end_secs).max(0.0);
            let nearest = main.iter().min_by(|a, b| gap(a).total_cmp(&gap(b))).map_or(turn.speaker, |other| other.speaker);
            SpeakerTurn { speaker: nearest, ..turn.clone() }
        })
        .collect()
}

/// Labels every system-audio segment with the voice that speaks for most of
/// it. Voices are numbered from 1 in the order they are first heard. Segments
/// that overlap no turn keep `None`.
pub fn assign_speakers(segments: &mut [TranscriptSegment], turns: &[SpeakerTurn]) {
    let mut order: Vec<usize> = (0..segments.len()).filter(|&index| segments[index].source == Source::System).collect();
    order.sort_by(|&a, &b| segments[a].start_secs.total_cmp(&segments[b].start_secs));

    let mut numbers: BTreeMap<u32, u32> = BTreeMap::new();
    for index in order {
        let segment = &mut segments[index];
        segment.speaker = dominant_voice(segment, turns).map(|voice| {
            let next = numbers.len() as u32 + 1;
            *numbers.entry(voice).or_insert(next)
        });
    }
}

fn dominant_voice(segment: &TranscriptSegment, turns: &[SpeakerTurn]) -> Option<u32> {
    let mut overlap_by_voice: BTreeMap<u32, f64> = BTreeMap::new();
    for turn in turns {
        let overlap = segment.end_secs.min(turn.end_secs) - segment.start_secs.max(turn.start_secs);
        if overlap > 0.0 {
            *overlap_by_voice.entry(turn.speaker).or_default() += overlap;
        }
    }
    overlap_by_voice
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(voice, _)| voice)
}

/// Names the user gave to the speakers of one meeting.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
#[serde(transparent)]
pub struct SpeakerNames(BTreeMap<u32, String>);

impl FromIterator<(u32, String)> for SpeakerNames {
    fn from_iter<I: IntoIterator<Item = (u32, String)>>(names: I) -> Self {
        Self(names.into_iter().collect())
    }
}

impl SpeakerNames {
    /// The label shown in front of a segment.
    pub fn label(&self, segment: &TranscriptSegment) -> String {
        match (segment.source, segment.speaker) {
            (Source::Microphone, _) => "Saya".into(),
            (Source::System, None) => "Lainnya".into(),
            (Source::System, Some(speaker)) => self.name(speaker),
        }
    }

    pub fn name(&self, speaker: u32) -> String {
        self.0.get(&speaker).cloned().unwrap_or_else(|| format!("Pembicara {speaker}"))
    }

    /// The names that were given, by speaker number.
    pub fn given(&self) -> impl Iterator<Item = (u32, &str)> {
        self.0.iter().map(|(&speaker, name)| (speaker, name.as_str()))
    }

    /// A blank name restores the default "Pembicara n".
    pub fn rename(&mut self, speaker: u32, name: &str) {
        match name.trim() {
            "" => self.0.remove(&speaker),
            trimmed => self.0.insert(speaker, trimmed.to_owned()),
        };
    }
}

/// Replaces a speaker label in text such as the minutes. Only whole labels
/// match, so renaming "Pembicara 1" leaves "Pembicara 10" alone.
pub fn replace_label(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() || old == new {
        return text.to_owned();
    }
    let mut replaced = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(old) {
        let (before, after) = (&rest[..position], &rest[position + old.len()..]);
        let whole = !before.chars().next_back().is_some_and(char::is_alphanumeric)
            && !after.chars().next().is_some_and(char::is_alphanumeric);
        replaced.push_str(before);
        replaced.push_str(if whole { new } else { old });
        rest = after;
    }
    replaced.push_str(rest);
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(source: Source, start_secs: f64, end_secs: f64) -> TranscriptSegment {
        TranscriptSegment { source, start_secs, end_secs, text: String::new(), speaker: None }
    }

    fn turn(start_secs: f64, end_secs: f64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn { start_secs, end_secs, speaker }
    }

    fn speakers(segments: &[TranscriptSegment]) -> Vec<Option<u32>> {
        segments.iter().map(|segment| segment.speaker).collect()
    }

    #[test]
    fn voices_are_numbered_in_the_order_they_are_first_heard() {
        let mut segments = [
            segment(Source::System, 0.0, 4.0),
            segment(Source::System, 5.0, 9.0),
            segment(Source::System, 10.0, 14.0),
        ];
        let turns = [turn(0.0, 4.5, 7), turn(4.5, 9.5, 3), turn(9.5, 15.0, 7)];
        assign_speakers(&mut segments, &turns);
        assert_eq!(speakers(&segments), [Some(1), Some(2), Some(1)]);
    }

    #[test]
    fn short_lived_voices_join_the_nearest_main_voice() {
        let turns = [turn(0.0, 100.0, 0), turn(100.0, 102.0, 5), turn(102.0, 200.0, 1), turn(300.0, 303.0, 6), turn(210.0, 250.0, 0)];
        let merged: Vec<u32> = merge_minor_voices(&turns).iter().map(|t| t.speaker).collect();
        // Voice 5 sits between 0 and 1 (tie: the first); voice 6 is closest to the turn ending at 250.
        assert_eq!(merged, [0, 0, 1, 0, 0]);
        // With only minor voices nothing is merged.
        let only = [turn(0.0, 2.0, 3), turn(2.0, 4.0, 4)];
        assert_eq!(merge_minor_voices(&only), only);
    }

    #[test]
    fn a_segment_goes_to_the_voice_that_speaks_for_most_of_it() {
        let mut segments = [segment(Source::System, 0.0, 10.0)];
        let turns = [turn(0.0, 3.0, 0), turn(3.0, 6.0, 1), turn(8.0, 10.0, 0)];
        assign_speakers(&mut segments, &turns);
        // Voice 0 speaks for 5 s in two turns, voice 1 for 3 s.
        assert_eq!(speakers(&segments), [Some(1)]);
        let mut both = [segment(Source::System, 0.0, 10.0), segment(Source::System, 20.0, 21.0)];
        assign_speakers(&mut both, &[turn(0.0, 3.0, 0), turn(3.0, 10.0, 1), turn(20.0, 21.0, 0)]);
        assert_eq!(speakers(&both), [Some(1), Some(2)]);
    }

    #[test]
    fn microphone_segments_are_never_labelled() {
        let mut segments = [segment(Source::Microphone, 0.0, 4.0), segment(Source::System, 0.0, 4.0)];
        assign_speakers(&mut segments, &[turn(0.0, 4.0, 0)]);
        assert_eq!(speakers(&segments), [None, Some(1)]);
    }

    #[test]
    fn a_segment_outside_every_turn_stays_unlabelled() {
        let mut segments = [segment(Source::System, 30.0, 34.0)];
        assign_speakers(&mut segments, &[turn(0.0, 4.0, 0)]);
        assert_eq!(speakers(&segments), [None]);
    }

    #[test]
    fn numbering_follows_time_even_when_segments_arrived_out_of_order() {
        let mut segments = [segment(Source::System, 10.0, 14.0), segment(Source::System, 0.0, 4.0)];
        assign_speakers(&mut segments, &[turn(0.0, 5.0, 9), turn(10.0, 15.0, 2)]);
        assert_eq!(speakers(&segments), [Some(2), Some(1)]);
    }

    #[test]
    fn labels_default_to_saya_lainnya_and_pembicara_n() {
        let names = SpeakerNames::default();
        let mut diarized = segment(Source::System, 0.0, 1.0);
        diarized.speaker = Some(2);
        assert_eq!(names.label(&segment(Source::Microphone, 0.0, 1.0)), "Saya");
        assert_eq!(names.label(&segment(Source::System, 0.0, 1.0)), "Lainnya");
        assert_eq!(names.label(&diarized), "Pembicara 2");
    }

    #[test]
    fn a_renamed_speaker_uses_the_given_name_until_it_is_cleared() {
        let mut names = SpeakerNames::default();
        names.rename(2, " Budi ");
        assert_eq!(names.name(2), "Budi");
        assert_eq!(names.name(1), "Pembicara 1");
        names.rename(2, "  ");
        assert_eq!(names.name(2), "Pembicara 2");
    }

    #[test]
    fn replacing_a_label_changes_every_whole_occurrence() {
        let text = "- [Pembicara 1] kirim laporan\n- Pembicara 1 dan Pembicara 2 setuju.";
        assert_eq!(
            replace_label(text, "Pembicara 1", "Budi"),
            "- [Budi] kirim laporan\n- Budi dan Pembicara 2 setuju."
        );
    }

    #[test]
    fn replacing_a_label_leaves_longer_labels_alone() {
        assert_eq!(replace_label("Pembicara 10, Pembicara 1.", "Pembicara 1", "Budi"), "Pembicara 10, Budi.");
        assert_eq!(replace_label("Budiman dan Budi", "Budi", "Ani"), "Budiman dan Ani");
    }
}
