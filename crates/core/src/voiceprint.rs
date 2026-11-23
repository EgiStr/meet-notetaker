//! Remembering who a voice belongs to: a voiceprint is the average speaker embedding of what one
//! person said. Naming a speaker in one meeting stores their voiceprint; later meetings name the
//! same voice automatically.

use std::collections::BTreeMap;

use polyvoice::embedder::{Embedder, ResNet34Native};
use polyvoice::models::ModelRegistry;

use crate::embed::cosine;
use crate::transcript::{Source, TranscriptSegment};

/// Cosine similarity from which two voiceprints are taken to be the same person.
pub const MATCH_THRESHOLD: f32 = 0.65;
/// Pieces shorter than this give unreliable embeddings.
const MIN_PIECE_SECS: f64 = 1.5;
/// Longest stretch embedded at once.
const MAX_PIECE_SECS: f64 = 12.0;
/// A voiceprint is built from this much speech at most; more only costs time.
const MAX_SPEECH_SECS: f64 = 45.0;

/// Turns audio into an embedding, so that tests need no model.
pub trait Embed {
    fn embed(&self, audio: &[f32]) -> Option<Vec<f32>>;
}

/// The speaker embedding model of the diarizer. Its files are downloaded on first use.
pub struct Resnet(ResNet34Native);

impl Resnet {
    pub fn load() -> Result<Self, String> {
        let registry = ModelRegistry::default().map_err(|error| error.to_string())?;
        let path = registry.ensure("resnet34_int8").map_err(|error| error.to_string())?;
        ResNet34Native::from_onnx_path(path).map(Self).map_err(|error| error.to_string())
    }
}

impl Embed for Resnet {
    fn embed(&self, audio: &[f32]) -> Option<Vec<f32>> {
        self.0.embed(audio).ok().filter(|vector| vector.iter().all(|value| value.is_finite()))
    }
}

/// A voiceprint of every numbered speaker of the system audio, from the speech the transcript says
/// each of them spoke. `samples` is 16 kHz mono and starts with the session.
pub fn speaker_prints(
    embedder: &dyn Embed,
    samples: &[f32],
    segments: &[TranscriptSegment],
    sample_rate: u32,
) -> BTreeMap<u32, Vec<f32>> {
    let mut by_speaker: BTreeMap<u32, Vec<&TranscriptSegment>> = BTreeMap::new();
    for segment in segments {
        if let (Source::System, Some(speaker)) = (segment.source, segment.speaker) {
            by_speaker.entry(speaker).or_default().push(segment);
        }
    }
    let rate = f64::from(sample_rate);
    let mut prints = BTreeMap::new();
    for (speaker, mut pieces) in by_speaker {
        pieces.sort_by(|a, b| (b.end_secs - b.start_secs).total_cmp(&(a.end_secs - a.start_secs)));
        let mut vectors = Vec::new();
        let mut used = 0.0;
        for piece in pieces {
            let length = (piece.end_secs - piece.start_secs).min(MAX_PIECE_SECS);
            if length < MIN_PIECE_SECS || used >= MAX_SPEECH_SECS {
                continue;
            }
            let start = (piece.start_secs * rate) as usize;
            let end = ((piece.start_secs + length) * rate) as usize;
            let Some(audio) = samples.get(start..end.min(samples.len())) else { continue };
            if let Some(vector) = embedder.embed(audio) {
                vectors.push(vector);
                used += length;
            }
        }
        if let Some(print) = average(&vectors) {
            prints.insert(speaker, print);
        }
    }
    prints
}

/// The normalized mean; `None` for no vectors or vectors of different sizes.
pub fn average(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vectors.first()?;
    if vectors.iter().any(|vector| vector.len() != first.len()) {
        return None;
    }
    let mut sum = vec![0.0f32; first.len()];
    for vector in vectors {
        for (total, value) in sum.iter_mut().zip(vector) {
            *total += value;
        }
    }
    let norm = sum.iter().map(|value| value * value).sum::<f32>().sqrt();
    (norm > 0.0).then(|| sum.iter().map(|value| value / norm).collect())
}

/// The name whose voiceprint is closest to `print`, if it is close enough. A name is given to one
/// voice at most; the voice that matches it best keeps it.
pub fn recognize(prints: &BTreeMap<u32, Vec<f32>>, known: &[(String, Vec<f32>)]) -> BTreeMap<u32, String> {
    let mut pairs: Vec<(f32, u32, &str)> = Vec::new();
    for (&speaker, print) in prints {
        for (name, stored) in known {
            let score = cosine(print, stored);
            if score >= MATCH_THRESHOLD {
                pairs.push((score, speaker, name));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut found: BTreeMap<u32, String> = BTreeMap::new();
    for (_, speaker, name) in pairs {
        if !found.contains_key(&speaker) && !found.values().any(|taken| taken == name) {
            found.insert(speaker, name.to_owned());
        }
    }
    found
}

/// Mixes a new voiceprint into a stored one, which weighs as much as `weight` meetings.
pub fn merge(stored: &[f32], weight: u32, new: &[f32]) -> Vec<f32> {
    let mixed: Vec<f32> = stored.iter().zip(new).map(|(old, new)| old * weight as f32 + new).collect();
    average(&[mixed]).unwrap_or_else(|| new.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ByLevel;
    impl Embed for ByLevel {
        /// "Voices" are distinguished by the sign of the samples.
        fn embed(&self, audio: &[f32]) -> Option<Vec<f32>> {
            Some(if audio[0] > 0.0 { vec![1.0, 0.1] } else { vec![0.1, 1.0] })
        }
    }

    fn segment(start: f64, end: f64, speaker: Option<u32>, source: Source) -> TranscriptSegment {
        TranscriptSegment { source, start_secs: start, end_secs: end, text: String::new(), speaker }
    }

    #[test]
    fn a_print_is_built_for_each_numbered_voice_from_long_enough_speech() {
        let mut samples = vec![1.0f32; 5 * 100];
        samples.extend(vec![-1.0f32; 5 * 100]);
        let segments = [
            segment(0.0, 5.0, Some(1), Source::System),
            segment(5.0, 5.5, Some(2), Source::System), // too short
            segment(5.0, 10.0, Some(2), Source::System),
            segment(0.0, 5.0, None, Source::Microphone),
        ];
        let prints = speaker_prints(&ByLevel, &samples, &segments, 100);
        assert_eq!(prints.keys().copied().collect::<Vec<_>>(), [1, 2]);
        assert!(prints[&1][0] > prints[&1][1]);
        assert!(prints[&2][1] > prints[&2][0]);
    }

    #[test]
    fn a_stored_name_goes_to_the_closest_voice_once() {
        let prints = BTreeMap::from([(1, vec![1.0, 0.0]), (2, vec![0.9, 0.2])]);
        let known = vec![("Budi".to_owned(), vec![1.0, 0.05]), ("Ani".to_owned(), vec![0.0, 1.0])];
        let found = recognize(&prints, &known);
        assert_eq!(found.get(&1).map(String::as_str), Some("Budi"));
        assert_eq!(found.get(&2), None); // Budi is taken; Ani is far away
    }

    #[test]
    fn merging_keeps_the_print_normalized_and_moves_it_toward_the_new_one() {
        let merged = merge(&[1.0, 0.0], 1, &[0.0, 1.0]);
        let norm: f32 = merged.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
        assert!((merged[0] - merged[1]).abs() < 1e-5);
        assert_eq!(average(&[vec![1.0], vec![1.0, 2.0]]), None);
    }
}
