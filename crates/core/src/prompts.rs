//! Builds the prompts for minutes (MoM) and question answering.
//! Pure text in, text out: no state and no network.

use crate::llm::Prompt;
use crate::speakers::SpeakerNames;
use crate::transcript::TranscriptSegment;

/// The shape of the minutes. Each template is a list of sections; the rules
/// that keep the minutes faithful to the transcript are the same for all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MomTemplate {
    #[default]
    General,
    Standup,
    OneOnOne,
    Sales,
    Retro,
}

impl MomTemplate {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "general" => Self::General,
            "standup" => Self::Standup,
            "one_on_one" => Self::OneOnOne,
            "sales" => Self::Sales,
            "retro" => Self::Retro,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Standup => "standup",
            Self::OneOnOne => "one_on_one",
            Self::Sales => "sales",
            Self::Retro => "retro",
        }
    }

    fn sections(self) -> &'static [&'static str] {
        match self {
            Self::General => &["Ringkasan", "Topik", "Keputusan", "Action Items", "Pertanyaan Terbuka"],
            Self::Standup => &["Kemarin", "Hari Ini", "Hambatan", "Action Items"],
            Self::OneOnOne => &["Ringkasan", "Kabar dan Masukan", "Tujuan dan Pengembangan", "Action Items"],
            Self::Sales => &["Ringkasan", "Kebutuhan Pelanggan", "Keberatan", "Langkah Berikutnya", "Action Items"],
            Self::Retro => &["Berjalan Baik", "Perlu Diperbaiki", "Pembelajaran", "Action Items"],
        }
    }

    fn system_prompt(self) -> String {
        let sections = self.sections();
        let headings: String = sections.iter().map(|section| format!("## {section}\n")).collect();
        format!(
            "Anda notulis rapat. Tulis notulen (MoM) dalam bahasa Indonesia, berformat Markdown, \
dengan tepat {} bagian berikut dan urutan ini:\n\n{headings}\n{MOM_RULES}",
            sections.len()
        )
    }
}

const MOM_RULES: &str = "\
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
suara dan bisa salah dengar. \"Saya\" adalah pengguna; label lain adalah peserta lain. Tulis teks \
biasa tanpa format Markdown (tanpa tanda * atau #); untuk daftar, awali tiap butir dengan \"- \".";

const NONE: &str = "(belum ada)";

/// Asks for the minutes to be brought up to date with the transcript added since they were written.
pub fn incremental_mom(
    template: MomTemplate,
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
    Prompt { system: template.system_prompt(), user }
}

/// Asks for the minutes to be written from scratch from the whole meeting.
pub fn final_mom(template: MomTemplate, user_notes: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let user = format!(
        "Catatan saya:\n{}\n\nTranskrip lengkap rapat:\n{}\n\nRapat sudah selesai. Susun MoM akhir \
         dari seluruh transkrip.",
        or_none(user_notes),
        format_transcript(segments, names),
    );
    Prompt { system: template.system_prompt(), user }
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

const EXPAND_SYSTEM: &str = "\
Anda membantu pengguna merapikan catatan singkatnya dari sebuah rapat. Ubah catatan itu menjadi \
catatan yang lengkap dalam bahasa Indonesia: tiap butir catatan dijabarkan dengan fakta dari \
transkrip (siapa, apa, angka, tenggat) yang berkaitan dengannya. Pertahankan urutan dan maksud \
catatan, dan jangan menambah hal yang tidak ada di transkrip. Transkrip berasal dari pengenalan \
suara dan bisa salah dengar. \"Saya\" adalah pengguna; label lain adalah peserta lain. Tulis teks \
biasa tanpa format Markdown (tanpa tanda * atau #); awali tiap butir dengan \"- \". Keluarkan \
hanya catatan yang sudah diperluas, tanpa kalimat pembuka atau penutup.";

const SUGGEST_SYSTEM: &str = "\
Anda membantu pengguna yang sedang mengikuti rapat. Usulkan 3 sampai 5 pertanyaan singkat dalam \
bahasa Indonesia yang berguna ditanyakan kepada asisten AI tentang rapat ini sampai sekarang, \
misalnya meminta ringkasan, siapa memegang tugas tertentu, apa yang belum diputuskan, atau apa \
risikonya. Pertanyaan harus bisa dijawab dari transkrip dan khusus untuk isi rapat ini, bukan \
pertanyaan umum. Tulis satu pertanyaan per baris, diawali \"- \". Keluarkan hanya daftar itu.";

/// How many of the latest transcript lines the question suggestions are based on.
const SUGGEST_TRANSCRIPT_LINES: usize = 60;

/// Asks for the user's short notes to be written out with facts from the transcript.
pub fn expand_notes(user_notes: &str, mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let user = format!(
        "Catatan singkat saya:
{}

MoM saat ini:
{}

Transkrip rapat sejauh ini:
{}

         Perluas catatan singkat saya.",
        or_none(user_notes),
        or_none(mom),
        format_transcript(segments, names),
    );
    Prompt { system: EXPAND_SYSTEM.into(), user }
}

/// Asks for questions worth putting to the assistant, from the minutes and the latest transcript.
pub fn suggest_questions(mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let latest = &segments[segments.len().saturating_sub(SUGGEST_TRANSCRIPT_LINES)..];
    let user = format!(
        "MoM saat ini:
{}

Transkrip terbaru:
{}

Usulkan pertanyaan.",
        or_none(mom),
        format_transcript(latest, names),
    );
    Prompt { system: SUGGEST_SYSTEM.into(), user }
}

/// The questions in an LLM reply that lists one per line, with or without a bullet or number.
pub fn parse_questions(reply: &str) -> Vec<String> {
    reply
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '-' | '*' | '.' | ')') || c.is_whitespace())
                .trim()
        })
        .filter(|question| question.ends_with('?'))
        .map(str::to_owned)
        .take(5)
        .collect()
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
            MomTemplate::General,
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
        let prompt = incremental_mom(MomTemplate::General, "", "  ", &[segment(Source::System, 0.0, "Halo.")], &no_names());
        assert!(prompt.user.starts_with("MoM saat ini:\n(belum ada)\n\nCatatan saya:\n(belum ada)"));
    }

    #[test]
    fn final_prompt_asks_for_minutes_of_the_whole_meeting() {
        let prompt = final_mom(MomTemplate::General, "", &[segment(Source::Microphone, 61.0, "Setuju.")], &no_names());
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

    #[test]
    fn each_template_asks_for_its_own_sections() {
        let standup = MomTemplate::Standup.system_prompt();
        assert!(standup.contains("## Hambatan") && !standup.contains("## Keputusan"));
        assert!(standup.contains("tepat 4 bagian"));
        assert!(MomTemplate::General.system_prompt().contains("## Pertanyaan Terbuka"));
    }

    #[test]
    fn template_ids_round_trip() {
        for template in
            [MomTemplate::General, MomTemplate::Standup, MomTemplate::OneOnOne, MomTemplate::Sales, MomTemplate::Retro]
        {
            assert_eq!(MomTemplate::from_id(template.id()), Some(template));
        }
        assert_eq!(MomTemplate::from_id("tidak ada"), None);
    }

    #[test]
    fn expand_prompt_carries_notes_and_transcript() {
        let prompt = expand_notes("tenggat?", "", &[segment(Source::System, 0.0, "Jumat.")], &no_names());
        assert!(prompt.user.starts_with("Catatan singkat saya:
tenggat?"));
        assert!(prompt.user.contains("[00:00] Lainnya: Jumat."));
    }

    #[test]
    fn suggestions_are_based_on_the_latest_transcript_only() {
        let segments: Vec<_> = (0..100).map(|i| segment(Source::System, f64::from(i), &format!("ucapan{i}x"))).collect();
        let prompt = suggest_questions("", &segments, &no_names());
        assert!(prompt.user.contains("ucapan99x") && prompt.user.contains("ucapan40x"));
        assert!(!prompt.user.contains("ucapan39x"));
    }

    #[test]
    fn questions_are_read_from_bulleted_or_numbered_lines() {
        let reply = "Berikut:
- Apa keputusan anggaran?
2. Siapa pemilik tugas?
* Kapan tenggatnya?
- bukan pertanyaan";
        assert_eq!(
            parse_questions(reply),
            ["Apa keputusan anggaran?", "Siapa pemilik tugas?", "Kapan tenggatnya?"]
        );
        assert!(parse_questions("").is_empty());
    }
}
