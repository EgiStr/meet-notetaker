//! Builds the prompts for minutes (MoM) and question answering.
//! Pure text in, text out: no state and no network.

use crate::llm::Prompt;
use crate::speakers::SpeakerNames;
use crate::transcript::TranscriptSegment;

const MOM_SYSTEM: &str = "\
Anda notulis rapat. Tulis notulen (MoM) dalam bahasa Indonesia, berformat Markdown, \
dengan tepat lima bagian berikut dan urutan ini:

## Ringkasan
## Topik
## Keputusan
## Action Items
## Pertanyaan Terbuka

Aturan:
- Hanya tulis hal yang didukung transkrip. Jangan mengarang nama, angka, atau tenggat.
- Isi tiap bagian berupa butir yang diawali \"- \". Bagian tanpa isi ditulis \"- (belum ada)\".
- Action item ditulis \"- [penanggung jawab] tugas (tenggat)\"; lewati bagian yang tidak disebut.
- Transkrip berasal dari pengenalan suara dan bisa salah dengar. \"Saya\" adalah pengguna \
aplikasi; \"Lainnya\", \"Pembicara 1\", \"Pembicara 2\", dan nama lain adalah peserta lain. \
Sebut peserta dengan label itu apa adanya.
- \"Catatan saya\" adalah petunjuk dari pengguna tentang apa yang penting.
- Keluarkan hanya MoM, tanpa kalimat pembuka atau penutup.";

const ANSWER_SYSTEM: &str = "\
Anda membantu pengguna memahami rapat yang sedang diikutinya. Jawab dalam bahasa Indonesia, \
singkat dan langsung, hanya berdasarkan transkrip dan MoM yang diberikan. Jika jawabannya \
tidak ada di sana, katakan bahwa hal itu belum dibahas. Transkrip berasal dari pengenalan \
suara dan bisa salah dengar. \"Saya\" adalah pengguna; label lain adalah peserta lain.";

const NONE: &str = "(belum ada)";

/// Asks for the minutes to be brought up to date with the transcript added since they were written.
pub fn incremental_mom(
    current_mom: &str,
    user_notes: &str,
    new_segments: &[TranscriptSegment],
    names: &SpeakerNames,
) -> Prompt {
    let user = format!(
        "MoM saat ini:\n{}\n\nCatatan saya:\n{}\n\nTranskrip baru sejak MoM itu ditulis:\n{}\n\n\
         Perbarui MoM: pertahankan isi yang masih benar, tambahkan hal baru, dan perbaiki yang \
         berubah. Tulis ulang MoM selengkapnya.",
        or_none(current_mom),
        or_none(user_notes),
        format_transcript(new_segments, names),
    );
    Prompt { system: MOM_SYSTEM.into(), user }
}

/// Asks for the minutes to be written from scratch from the whole meeting.
pub fn final_mom(user_notes: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let user = format!(
        "Catatan saya:\n{}\n\nTranskrip lengkap rapat:\n{}\n\nRapat sudah selesai. Susun MoM akhir \
         dari seluruh transkrip.",
        or_none(user_notes),
        format_transcript(segments, names),
    );
    Prompt { system: MOM_SYSTEM.into(), user }
}

pub fn answer(question: &str, mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let user = format!(
        "MoM saat ini:\n{}\n\nTranskrip rapat sejauh ini:\n{}\n\nPertanyaan: {}",
        or_none(mom),
        format_transcript(segments, names),
        question.trim(),
    );
    Prompt { system: ANSWER_SYSTEM.into(), user }
}

/// One line per segment: `[mm:ss] Saya: text`.
pub fn format_transcript(segments: &[TranscriptSegment], names: &SpeakerNames) -> String {
    if segments.is_empty() {
        return NONE.into();
    }
    segments
        .iter()
        .map(|segment| {
            let whole = segment.start_secs.max(0.0) as u64;
            format!("[{:02}:{:02}] {}: {}", whole / 60, whole % 60, names.label(segment), segment.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn or_none(text: &str) -> &str {
    match text.trim() {
        "" => NONE,
        trimmed => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Source;

    fn segment(source: Source, start_secs: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment { source, start_secs, end_secs: start_secs + 1.0, text: text.into(), speaker: None }
    }

    fn no_names() -> SpeakerNames {
        SpeakerNames::default()
    }

    #[test]
    fn diarized_segments_carry_the_speaker_name() {
        let mut names = SpeakerNames::default();
        names.rename(2, "Budi");
        let mut first = segment(Source::System, 0.0, "Halo.");
        first.speaker = Some(1);
        let mut second = segment(Source::System, 3.0, "Hai.");
        second.speaker = Some(2);
        assert_eq!(format_transcript(&[first, second], &names), "[00:00] Pembicara 1: Halo.\n[00:03] Budi: Hai.");
    }

    #[test]
    fn transcript_lines_carry_clock_and_speaker() {
        let segments = [
            segment(Source::Microphone, 5.4, "Selamat pagi."),
            segment(Source::System, 125.0, "Pagi, kita mulai."),
        ];
        assert_eq!(
            format_transcript(&segments, &no_names()),
            "[00:05] Saya: Selamat pagi.\n[02:05] Lainnya: Pagi, kita mulai."
        );
    }

    #[test]
    fn empty_transcript_is_marked_as_none() {
        assert_eq!(format_transcript(&[], &no_names()), "(belum ada)");
    }

    #[test]
    fn incremental_prompt_contains_current_mom_notes_and_new_transcript() {
        let prompt = incremental_mom(
            "## Ringkasan\n- Rapat anggaran",
            "fokus ke tenggat",
            &[segment(Source::System, 0.0, "Tenggatnya Jumat.")],
            &no_names(),
        );
        assert!(prompt.user.contains("## Ringkasan\n- Rapat anggaran"));
        assert!(prompt.user.contains("fokus ke tenggat"));
        assert!(prompt.user.contains("[00:00] Lainnya: Tenggatnya Jumat."));
        assert!(prompt.system.contains("## Pertanyaan Terbuka"));
    }

    #[test]
    fn first_incremental_prompt_marks_missing_mom_and_notes() {
        let prompt = incremental_mom("", "  ", &[segment(Source::System, 0.0, "Halo.")], &no_names());
        assert!(prompt.user.starts_with("MoM saat ini:\n(belum ada)\n\nCatatan saya:\n(belum ada)"));
    }

    #[test]
    fn final_prompt_asks_for_minutes_of_the_whole_meeting() {
        let prompt = final_mom("", &[segment(Source::Microphone, 61.0, "Setuju.")], &no_names());
        assert!(prompt.user.contains("Transkrip lengkap rapat:\n[01:01] Saya: Setuju."));
        assert!(!prompt.user.contains("MoM saat ini"));
    }

    #[test]
    fn answer_prompt_contains_question_mom_and_transcript() {
        let segments = [segment(Source::System, 0.0, "Jumat.")];
        let prompt = answer(" Kapan tenggatnya? ", "## Ringkasan\n- X", &segments, &no_names());
        assert!(prompt.user.ends_with("Pertanyaan: Kapan tenggatnya?"));
        assert!(prompt.user.contains("## Ringkasan\n- X"));
        assert!(prompt.user.contains("[00:00] Lainnya: Jumat."));
    }
}
