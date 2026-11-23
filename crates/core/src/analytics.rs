//! Who spoke how much: talk time, share and habits per speaker, from the transcript.

use crate::speakers::SpeakerNames;
use crate::transcript::TranscriptSegment;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TalkStat {
    pub name: String,
    pub seconds: f64,
    /// Share of all speech, 0 to 1.
    pub share: f64,
    pub lines: usize,
    pub words: usize,
    pub questions: usize,
    /// The longest line, as a measure of monologue.
    pub longest_secs: f64,
    pub words_per_minute: f64,
}

/// One entry per speaker, the one who spoke most first. Lines that are far apart in time but have
/// no gap between them are not merged: a line is what the transcriber returned.
pub fn talk_stats(segments: &[TranscriptSegment], names: &SpeakerNames) -> Vec<TalkStat> {
    let mut stats: Vec<TalkStat> = Vec::new();
    for segment in segments {
        let name = names.label(segment);
        let index = match stats.iter().position(|stat| stat.name == name) {
            Some(index) => index,
            None => {
                stats.push(TalkStat {
                    name,
                    seconds: 0.0,
                    share: 0.0,
                    lines: 0,
                    words: 0,
                    questions: 0,
                    longest_secs: 0.0,
                    words_per_minute: 0.0,
                });
                stats.len() - 1
            }
        };
        let stat = &mut stats[index];
        let length = (segment.end_secs - segment.start_secs).max(0.0);
        stat.seconds += length;
        stat.lines += 1;
        stat.words += segment.text.split_whitespace().count();
        stat.questions += segment.text.matches('?').count();
        stat.longest_secs = stat.longest_secs.max(length);
    }
    let total: f64 = stats.iter().map(|stat| stat.seconds).sum();
    for stat in &mut stats {
        stat.share = if total > 0.0 { stat.seconds / total } else { 0.0 };
        stat.words_per_minute = if stat.seconds > 0.0 { stat.words as f64 / (stat.seconds / 60.0) } else { 0.0 };
    }
    stats.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Source;

    fn line(source: Source, start: f64, end: f64, text: &str, speaker: Option<u32>) -> TranscriptSegment {
        TranscriptSegment { source, start_secs: start, end_secs: end, text: text.into(), speaker }
    }

    #[test]
    fn time_share_and_habits_are_counted_per_label() {
        let mut names = SpeakerNames::default();
        names.rename(1, "Budi");
        let segments = [
            line(Source::Microphone, 0.0, 10.0, "Halo semuanya, apa kabar?", None),
            line(Source::System, 10.0, 40.0, "Baik, kita mulai. Siapa yang siap? Ada kendala?", Some(1)),
            line(Source::System, 40.0, 60.0, "Satu dua tiga empat", Some(1)),
            line(Source::System, 60.0, 70.0, "Tidak jelas", None),
        ];
        let stats = talk_stats(&segments, &names);
        assert_eq!(stats.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Budi", "Saya", "Lainnya"]);
        assert_eq!(stats[0].seconds, 50.0);
        assert_eq!(stats[0].lines, 2);
        assert_eq!(stats[0].questions, 2);
        assert_eq!(stats[0].longest_secs, 30.0);
        assert!((stats[0].share - 50.0 / 70.0).abs() < 1e-9);
        assert_eq!(stats[1].questions, 1);
        assert!((stats[0].words_per_minute - 12.0 / (50.0 / 60.0)).abs() < 1e-9);
    }

    #[test]
    fn an_empty_transcript_has_no_stats() {
        assert!(talk_stats(&[], &SpeakerNames::default()).is_empty());
    }
}
