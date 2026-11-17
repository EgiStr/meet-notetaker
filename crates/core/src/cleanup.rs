//! Keeps a chunked transcript clean and consistent, whatever provider produced it.
//!
//! Practices applied here (Whisper-style models transcribe each chunk on its own):
//! - A short prompt with the user's glossary and the tail of the previous text of the same
//!   stream, so names and terms are spelled the same way from chunk to chunk.
//! - Phrases the model invents for noise or silence ("Thanks for watching", "Terima kasih telah
//!   menonton") are dropped, as are exact repeats of the previous line and echoes of the prompt.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::transcript::{AudioChunk, Source, TranscribeError, Transcriber, TranscriptSegment};

/// How much of the previous text is handed to the model as context.
const CONTEXT_CHARS: usize = 200;
/// An identical line this close to the previous one is a repetition loop, not speech.
const REPEAT_WINDOW_SECS: f64 = 30.0;
/// Short lines only; a long sentence that contains one of these words is real speech.
const PHANTOM_MAX_CHARS: usize = 70;

const PHANTOMS: [&str; 14] = [
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "subtitles by",
    "subtitle by",
    "amara.org",
    "terima kasih telah menonton",
    "terima kasih sudah menonton",
    "terima kasih atas perhatiannya",
    "jangan lupa subscribe",
    "subtitled by",
    "sampai jumpa di video berikutnya",
    "sampai jumpa di video selanjutnya",
    "www.",
];

fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_phantom(text: &str) -> bool {
    if text.chars().count() > PHANTOM_MAX_CHARS {
        return false;
    }
    let text = text.to_lowercase();
    PHANTOMS.iter().any(|phrase| text.contains(phrase))
}

#[derive(Default)]
struct Stream {
    /// The last kept text, for context and for repeat detection.
    tail: String,
    last: String,
    last_end: f64,
}

/// Wraps a transcriber with the glossary prompt and the cleanup above.
pub struct CleaningTranscriber {
    inner: Arc<dyn Transcriber>,
    glossary: String,
    streams: Mutex<HashMap<&'static str, Stream>>,
}

fn key(source: Source) -> &'static str {
    match source {
        Source::Microphone => "microphone",
        Source::System => "system",
    }
}

impl CleaningTranscriber {
    /// `glossary`: names and terms that should be spelled as given, separated by commas or lines.
    pub fn new(inner: Arc<dyn Transcriber>, glossary: &str) -> Self {
        let glossary = glossary.split([',', '\n']).map(str::trim).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", ");
        Self { inner, glossary, streams: Mutex::new(HashMap::new()) }
    }

    fn prompt(&self, source: Source) -> String {
        let streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let tail = streams.get(key(source)).map_or("", |stream| stream.tail.as_str());
        match (self.glossary.is_empty(), tail.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!("Istilah: {}.", self.glossary),
            (true, false) => tail.to_owned(),
            (false, false) => format!("Istilah: {}. {}", self.glossary, tail),
        }
    }

    fn keep(&self, source: Source, segments: Vec<TranscriptSegment>, prompt: &str) -> Vec<TranscriptSegment> {
        let echo = normalized(prompt);
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let stream = streams.entry(key(source)).or_default();
        let mut kept = Vec::new();
        for segment in segments {
            let text = normalized(&segment.text);
            let recent = segment.start_secs - stream.last_end < REPEAT_WINDOW_SECS;
            let repeated = recent && text == stream.last;
            let echoed = recent && text.len() >= 8 && echo.contains(&text);
            if text.is_empty() || is_phantom(&segment.text) || repeated || echoed {
                continue;
            }
            stream.last = text;
            stream.last_end = segment.end_secs;
            stream.tail.push(' ');
            stream.tail.push_str(segment.text.trim());
            let excess = stream.tail.chars().count().saturating_sub(CONTEXT_CHARS);
            stream.tail = stream.tail.chars().skip(excess).collect::<String>().trim().to_owned();
            kept.push(segment);
        }
        kept
    }
}

impl Transcriber for CleaningTranscriber {
    fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        let prompt = self.prompt(chunk.source);
        let segments = self.inner.transcribe_with_prompt(chunk, &prompt)?;
        Ok(self.keep(chunk.source, segments, &prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the canned lines and remembers the prompts it was given.
    struct Canned {
        lines: Mutex<Vec<Vec<&'static str>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl Transcriber for Canned {
        fn transcribe(&self, chunk: &AudioChunk) -> Result<Vec<TranscriptSegment>, TranscribeError> {
            self.transcribe_with_prompt(chunk, "")
        }

        fn transcribe_with_prompt(&self, chunk: &AudioChunk, prompt: &str) -> Result<Vec<TranscriptSegment>, TranscribeError> {
            self.prompts.lock().unwrap().push(prompt.to_owned());
            let lines = self.lines.lock().unwrap().remove(0);
            Ok(lines
                .into_iter()
                .map(|text| TranscriptSegment {
                    source: chunk.source,
                    start_secs: chunk.start_secs,
                    end_secs: chunk.start_secs + 2.0,
                    text: text.into(),
                    speaker: None,
                })
                .collect())
        }
    }

    fn chunk(start_secs: f64) -> AudioChunk {
        AudioChunk { source: Source::System, start_secs, samples: Vec::new() }
    }

    fn cleaner(lines: Vec<Vec<&'static str>>, glossary: &str) -> (CleaningTranscriber, Arc<Canned>) {
        let inner = Arc::new(Canned { lines: Mutex::new(lines), prompts: Mutex::new(Vec::new()) });
        (CleaningTranscriber::new(inner.clone(), glossary), inner)
    }

    fn texts(segments: Vec<TranscriptSegment>) -> Vec<String> {
        segments.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn phrases_invented_for_silence_are_dropped() {
        let (cleaner, _) = cleaner(vec![vec!["Terima kasih telah menonton!", "Kita bahas anggaran Kubernetes bulan depan."]], "");
        assert_eq!(texts(cleaner.transcribe(&chunk(0.0)).unwrap()), vec!["Kita bahas anggaran Kubernetes bulan depan."]);
    }

    #[test]
    fn a_line_repeated_right_away_is_kept_once() {
        let (cleaner, _) = cleaner(vec![vec!["Oke, lanjut."], vec!["Oke lanjut"], vec!["Oke, lanjut."]], "");
        assert_eq!(cleaner.transcribe(&chunk(0.0)).unwrap().len(), 1);
        assert!(cleaner.transcribe(&chunk(5.0)).unwrap().is_empty());
        // Much later the same words are speech again.
        assert_eq!(cleaner.transcribe(&chunk(100.0)).unwrap().len(), 1);
    }

    #[test]
    fn the_prompt_carries_the_glossary_and_the_previous_text_of_the_same_stream() {
        let (cleaner, inner) = cleaner(vec![vec!["Budi membahas rilis."], vec!["Lalu Siti setuju."]], "Kubernetes, Budi\nSiti");
        cleaner.transcribe(&chunk(0.0)).unwrap();
        cleaner.transcribe(&chunk(10.0)).unwrap();
        let prompts = inner.prompts.lock().unwrap();
        assert_eq!(prompts[0], "Istilah: Kubernetes, Budi, Siti.");
        assert_eq!(prompts[1], "Istilah: Kubernetes, Budi, Siti. Budi membahas rilis.");
    }

    #[test]
    fn text_that_only_echoes_the_prompt_is_dropped() {
        let (cleaner, _) = cleaner(vec![vec!["Budi membahas rilis."], vec!["Budi membahas rilis", "Siti setuju dengan jadwalnya."]], "");
        cleaner.transcribe(&chunk(0.0)).unwrap();
        assert_eq!(texts(cleaner.transcribe(&chunk(10.0)).unwrap()), vec!["Siti setuju dengan jadwalnya."]);
    }
}
