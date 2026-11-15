//! Calls the real LLM provider. Skipped by default; run with
//! `cargo test -p notetaker-core --test live_llm -- --ignored --nocapture`
//! after setting LLM_API_KEY (and optionally LLM_BASE_URL, LLM_MODEL).

use std::sync::Arc;

use notetaker_core::notes::MeetingNotes;
use notetaker_core::openai_llm::{OpenAiLlm, OpenAiLlmSettings};
use notetaker_core::transcript::{Source, TranscriptSegment};

const TRANSCRIPT: &[(Source, &str)] = &[
    (Source::Microphone, "Selamat pagi, kita mulai rapat peluncuran aplikasi kasir."),
    (Source::System, "Pagi. Dari tim backend, API pembayaran sudah selesai dan lolos uji beban."),
    (Source::System, "Tapi integrasi QRIS masih menunggu sertifikat dari bank, paling cepat Rabu depan."),
    (Source::Microphone, "Kalau begitu peluncuran kita undur dari tanggal 20 ke tanggal 27 November. Setuju?"),
    (Source::System, "Setuju. Budi akan menyiapkan materi pelatihan kasir sebelum tanggal 25."),
    (Source::System, "Satu hal yang belum jelas, siapa yang menanggung biaya printer struk di cabang?"),
    (Source::Microphone, "Itu saya tanyakan ke bagian keuangan, belum ada jawabannya."),
];

fn settings() -> OpenAiLlmSettings {
    let env = |name: &str| std::env::var(name).ok();
    OpenAiLlmSettings {
        base_url: env("LLM_BASE_URL")
            .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta/openai".into()),
        api_key: env("LLM_API_KEY").expect("LLM_API_KEY is not set"),
        model: env("LLM_MODEL").unwrap_or_else(|| "gemini-2.5-flash".into()),
    }
}

#[test]
#[ignore = "calls the real LLM provider"]
fn minutes_and_answers_come_from_the_transcript() {
    let notes = MeetingNotes::new(Arc::new(OpenAiLlm::new(settings()).unwrap()));
    let segments = TRANSCRIPT.iter().enumerate().map(|(index, (source, text))| TranscriptSegment {
        source: *source,
        start_secs: index as f64 * 12.0,
        end_secs: index as f64 * 12.0 + 10.0,
        text: (*text).into(),
        speaker: None,
    });

    let mut segments = segments.collect::<Vec<_>>();
    let late = segments.split_off(4);
    segments.into_iter().for_each(|segment| notes.add_segment(segment));
    let first = notes.update_mom().unwrap().unwrap();
    println!("--- MoM setelah 4 segmen ---\n{first}\n");

    late.into_iter().for_each(|segment| notes.add_segment(segment));
    let second = notes.update_mom().unwrap().unwrap();
    println!("--- MoM setelah 7 segmen ---\n{second}\n");

    for heading in ["## Ringkasan", "## Topik", "## Keputusan", "## Action Items", "## Pertanyaan Terbuka"] {
        assert!(second.contains(heading), "missing {heading}");
    }
    assert!(second.contains("27"), "the new launch date is missing");
    assert!(second.contains("Budi"), "the action item owner is missing");

    let answer = notes.answer("Peluncuran diundur ke tanggal berapa, dan kenapa?").unwrap();
    println!("--- Jawaban ---\n{answer}\n");
    assert!(answer.contains("27"));

    let unknown = notes.answer("Berapa anggaran iklan yang disetujui?").unwrap();
    println!("--- Jawaban di luar transkrip ---\n{unknown}\n");

    let rebuilt = notes.rebuild_mom().unwrap().unwrap();
    println!("--- MoM akhir ---\n{rebuilt}");
    assert!(rebuilt.contains("## Keputusan"));
}
