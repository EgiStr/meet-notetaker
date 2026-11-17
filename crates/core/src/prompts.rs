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

const CATCH_UP_SYSTEM: &str = "\
Anda mendampingi pengguna yang baru bergabung atau sempat tidak menyimak sebuah rapat yang sedang \
berlangsung. Dalam bahasa Indonesia yang singkat dan jelas, bantu ia langsung paham situasinya \
dari transkrip. Tulis tepat empat bagian dengan judul ini, masing-masing satu sampai tiga butir \
diawali \"- \":\n\
Sedang dibahas:\n\
Sudah diputuskan atau disepakati:\n\
Yang diminta dari saya atau perlu saya ketahui:\n\
Istilah atau konteks penting:\n\
Hanya berdasarkan transkrip, jangan menebak; jika suatu bagian tidak ada, tulis \"- (belum ada)\". \
\"Saya\" adalah pengguna. Transkrip berasal dari pengenalan suara dan bisa salah dengar. Teks biasa, \
tanpa format Markdown (tanpa tanda * atau #).";

const EXPLAIN_SYSTEM: &str = "\
Anda membantu pengguna yang sedang mengikuti rapat dan kurang paham satu ucapan. Jelaskan dalam \
bahasa Indonesia yang sederhana, paling banyak empat kalimat: apa maksud ucapan itu dalam konteks \
rapat ini, dan arti istilah, singkatan, atau nama yang mungkin asing. Gunakan percakapan di sekitarnya \
sebagai konteks dan katakan jika konteksnya belum cukup. Transkrip bisa salah dengar; jika sebuah kata \
tampak keliru, sebutkan kemungkinan yang dimaksud. Teks biasa, tanpa format Markdown.";

const TITLE_SYSTEM: &str = "\
Beri judul singkat untuk rapat berdasarkan MoM-nya: paling banyak 6 kata, bahasa Indonesia, \
menyebut topik utama, tanpa tanda kutip dan tanpa kata \"Rapat\" di awal. Keluarkan hanya judulnya.";

const MAX_TITLE_CHARS: usize = 60;

/// Asks for a short title for a meeting from its minutes.
pub fn title(mom: &str) -> Prompt {
    Prompt { system: TITLE_SYSTEM.into(), user: format!("MoM:\n{}\n\nJudul:", or_none(mom)) }
}

/// The title in an LLM reply: the first line without quotes, bullets or a "Judul:" label, cut short.
pub fn parse_title(reply: &str) -> Option<String> {
    let line = reply.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = line.trim_start_matches(|c: char| matches!(c, '-' | '*' | '#') || c.is_whitespace());
    let line = line.strip_prefix("Judul:").unwrap_or(line);
    let title = line.trim().trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '*')).trim();
    if title.is_empty() {
        return None;
    }
    Some(title.chars().take(MAX_TITLE_CHARS).collect::<String>().trim_end().to_owned())
}

const FOLLOW_UP_SYSTEM: &str = "\
Anda menulis draf email tindak lanjut rapat atas nama pengguna (\"Saya\" di transkrip). Tulis dalam \
bahasa Indonesia yang sopan dan ringkas, siap dikirim ke seluruh peserta: baris pertama \"Subjek: \
…\", lalu sapaan, ringkasan singkat hasil rapat, keputusan, dan daftar tindak lanjut dengan \
penanggung jawab dan tenggat bila disebut. Hanya gunakan fakta dari MoM dan transkrip; jangan \
mengarang nama, angka, atau tanggal, dan tulis \"(belum ditentukan)\" bila tidak ada. Akhiri dengan \
salam penutup tanpa nama. Teks biasa tanpa format Markdown (tanpa tanda * atau #); untuk daftar, \
awali tiap butir dengan \"- \".";

/// Asks for a ready-to-send follow-up email from the finished minutes.
pub fn follow_up_email(user_notes: &str, mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let user = format!(
        "MoM:\n{}\n\nCatatan saya:\n{}\n\nTranskrip:\n{}\n\nTulis draf email tindak lanjut.",
        or_none(mom),
        or_none(user_notes),
        format_transcript(segments, names),
    );
    Prompt { system: FOLLOW_UP_SYSTEM.into(), user }
}

/// How many lines before and after the line to explain are given as context.
const EXPLAIN_CONTEXT_LINES: usize = 12;

/// A recap of the last `window_secs` of the meeting, for someone who just joined or looked away.
/// `segments` must be in time order.
pub fn catch_up(window_secs: f64, mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let end = segments.iter().map(|segment| segment.end_secs).fold(0.0, f64::max);
    let recent: Vec<TranscriptSegment> =
        segments.iter().filter(|segment| segment.end_secs >= end - window_secs).cloned().collect();
    let user = format!(
        "MoM sejauh ini (latar belakang):\n{}\n\nTranskrip {} menit terakhir:\n{}\n\nBantu saya menyusul.",
        or_none(mom),
        (window_secs / 60.0).round().max(1.0),
        format_transcript(&recent, names),
    );
    Prompt { system: CATCH_UP_SYSTEM.into(), user }
}

/// Explains the line that starts at `at_secs`, using the lines around it. `segments` must be in time order.
pub fn explain(text: &str, at_secs: f64, mom: &str, segments: &[TranscriptSegment], names: &SpeakerNames) -> Prompt {
    let position = segments.iter().position(|segment| segment.start_secs >= at_secs).unwrap_or(segments.len());
    let from = position.saturating_sub(EXPLAIN_CONTEXT_LINES);
    let to = (position + EXPLAIN_CONTEXT_LINES + 1).min(segments.len());
    let user = format!(
        "MoM sejauh ini:\n{}\n\nPercakapan di sekitarnya:\n{}\n\nUcapan yang saya tidak paham: \"{}\"",
        or_none(mom),
        format_transcript(&segments[from..to], names),
        text.trim(),
    );
    Prompt { system: EXPLAIN_SYSTEM.into(), user }
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
    fn a_catch_up_covers_only_the_recent_window_and_asks_for_four_parts() {
        let segments = [
            segment(Source::System, 0.0, "Pembukaan."),
            segment(Source::System, 590.0, "Kita bahas anggaran."),
            segment(Source::System, 600.0, "Disetujui."),
        ];
        let prompt = catch_up(300.0, "## Ringkasan\n- Rapat", &segments, &no_names());
        assert!(!prompt.user.contains("Pembukaan."));
        assert!(prompt.user.contains("Transkrip 5 menit terakhir:\n[09:50] Lainnya: Kita bahas anggaran.\n[10:00] Lainnya: Disetujui."));
        assert!(prompt.user.contains("- Rapat"));
        for heading in ["Sedang dibahas:", "Sudah diputuskan atau disepakati:", "Yang diminta dari saya", "Istilah atau konteks penting:"] {
            assert!(prompt.system.contains(heading), "missing {heading}");
        }
    }

    #[test]
    fn explaining_a_line_shows_the_lines_around_it_but_not_far_ones() {
        let segments: Vec<_> = (0..40).map(|i| segment(Source::System, f64::from(i) * 10.0, &format!("kalimat {i}"))).collect();
        let prompt = explain(" kalimat 20 ", 200.0, "", &segments, &no_names());
        assert!(prompt.user.contains("kalimat 20") && prompt.user.contains("kalimat 8") && prompt.user.contains("kalimat 32"));
        assert!(!prompt.user.contains("kalimat 7\n") && !prompt.user.contains("kalimat 33"));
        assert!(prompt.user.ends_with("Ucapan yang saya tidak paham: \"kalimat 20\""));
    }

    #[test]
    fn a_title_is_the_first_clean_line_of_the_reply() {
        assert_eq!(parse_title("\n  \"Anggaran dan Jadwal Rilis\"\nPenjelasan"), Some("Anggaran dan Jadwal Rilis".into()));
        assert_eq!(parse_title("Judul: **Evaluasi Vendor**"), Some("Evaluasi Vendor".into()));
        assert_eq!(parse_title("- Sprint retro"), Some("Sprint retro".into()));
        assert_eq!(parse_title(&"a".repeat(100)).unwrap().chars().count(), 60);
        assert_eq!(parse_title("  \n "), None);
        assert_eq!(parse_title("\"\""), None);
        assert!(title("").user.contains("(belum ada)"));
    }

    #[test]
    fn the_follow_up_prompt_has_minutes_notes_and_transcript() {
        let prompt = follow_up_email("kirim sebelum Jumat", "## Ringkasan\n- Anggaran", &[segment(Source::System, 0.0, "Disetujui.")], &no_names());
        assert!(prompt.user.contains("- Anggaran") && prompt.user.contains("kirim sebelum Jumat"));
        assert!(prompt.user.contains("[00:00] Lainnya: Disetujui."));
        assert!(prompt.system.contains("Subjek:"));
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
