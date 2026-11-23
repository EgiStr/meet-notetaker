//! A clip: a short stretch of a meeting around a bookmark, as one HTML file with the audio and
//! the words, to send to someone who was not there.

use crate::export::clock;
use crate::report::escape;
use crate::speakers::SpeakerNames;
use crate::transcript::TranscriptSegment;

/// How much of the meeting a clip shows before and after the bookmark.
pub const SECS_BEFORE: f64 = 15.0;
pub const SECS_AFTER: f64 = 45.0;

/// The range of a clip around `at_secs`, kept inside the meeting.
pub fn range(at_secs: f64, duration_secs: f64) -> (f64, f64) {
    let start = (at_secs - SECS_BEFORE).max(0.0);
    let end = (at_secs + SECS_AFTER).min(duration_secs.max(at_secs));
    (start, end.max(start))
}

/// Adds two streams of the same recording; the shorter one counts as silence at its end.
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    (0..a.len().max(b.len()))
        .map(|index| (a.get(index).copied().unwrap_or(0.0) + b.get(index).copied().unwrap_or(0.0)).clamp(-1.0, 1.0))
        .collect()
}

/// 16-bit mono WAV file bytes.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, hound::Error> {
    let spec = hound::WavSpec { channels: 1, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for sample in samples {
            writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16) | (u32::from(*group.get(1).unwrap_or(&0)) << 8) | u32::from(*group.get(2).unwrap_or(&0));
        for position in 0..4 {
            if position <= group.len() {
                out.push(ALPHABET[((n >> (18 - 6 * position)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The page of a clip. `audio` is what `wav_bytes` returned for the range, if audio was kept.
pub fn clip_html(
    meeting_title: &str,
    label: &str,
    start_secs: f64,
    end_secs: f64,
    segments: &[TranscriptSegment],
    names: &SpeakerNames,
    audio: Option<&[u8]>,
) -> String {
    let lines: String = segments
        .iter()
        .filter(|segment| segment.end_secs > start_secs && segment.start_secs < end_secs)
        .map(|segment| {
            format!(
                "<p><span class=t>{}</span> <b>{}</b> {}</p>\n",
                clock(segment.start_secs),
                escape(&names.label(segment)),
                escape(&segment.text)
            )
        })
        .collect();
    let player = audio
        .map(|bytes| format!("<audio controls src=\"data:audio/wav;base64,{}\"></audio>", base64(bytes)))
        .unwrap_or_default();
    format!(
        "<!doctype html><html lang=id><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
<title>{label}</title><style>body{{font:16px/1.6 system-ui,sans-serif;max-width:42rem;margin:2rem auto;padding:0 1rem;color:#18181b}}\
h1{{font-size:1.3rem;margin:0}}small{{color:#71717a}}audio{{width:100%;margin:1rem 0}}.t{{color:#71717a;font:12px monospace}}</style>\
<h1>{label}</h1><small>{meeting}, {from}–{to}</small>{player}{lines}</html>",
        label = escape(label),
        meeting = escape(meeting_title),
        from = clock(start_secs),
        to = clock(end_secs),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Source;

    #[test]
    fn a_clip_stays_inside_the_meeting() {
        assert_eq!(range(100.0, 1000.0), (85.0, 145.0));
        assert_eq!(range(5.0, 1000.0), (0.0, 50.0));
        assert_eq!(range(990.0, 1000.0), (975.0, 1000.0));
    }

    #[test]
    fn base64_matches_the_standard_examples() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn mixing_pads_the_shorter_stream_and_never_clips() {
        assert_eq!(mix(&[0.5, 0.9], &[0.5]), [1.0, 0.9]);
    }

    #[test]
    fn the_page_has_the_lines_in_range_with_names_and_escaped_text() {
        let segment = |start: f64, text: &str| TranscriptSegment {
            source: Source::System,
            start_secs: start,
            end_secs: start + 4.0,
            text: text.into(),
            speaker: Some(1),
        };
        let mut names = SpeakerNames::default();
        names.rename(1, "Budi");
        let segments = [segment(0.0, "terlalu awal"), segment(20.0, "a < b"), segment(200.0, "terlalu akhir")];
        let html = clip_html("Rapat", "Keputusan", 15.0, 60.0, &segments, &names, Some(b"abc"));
        assert!(html.contains("<b>Budi</b> a &lt; b"));
        assert!(!html.contains("terlalu"));
        assert!(html.contains("data:audio/wav;base64,YWJj"));
    }
}
