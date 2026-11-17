//! Searches meetings by meaning with a real embedding provider. Skipped by default; run with
//! `cargo test -p notetaker-core --test live_embeddings -- --ignored --nocapture`
//! after setting EMBEDDING_API_KEY (and optionally EMBEDDING_BASE_URL, EMBEDDING_MODEL).

use std::sync::Mutex;

use notetaker_core::embed::{default_model, OpenAiEmbedder};
use notetaker_core::notes::NotesSnapshot;
use notetaker_core::semantic::{index_pending, search};
use notetaker_core::store::Store;
use notetaker_core::transcript::{Source, TranscriptSegment};

const MEETINGS: [(&str, &[&str]); 4] = [
    ("Rapat infrastruktur", &["Tagihan cloud bulan ini naik tiga puluh persen karena traffic.", "Kita sepakat memindahkan database ke server yang lebih murah."]),
    ("Rapat rekrutmen", &["Kita butuh dua engineer backend baru sebelum kuartal depan.", "Proses wawancara akan dipercepat menjadi seminggu."]),
    ("Rapat peluncuran", &["Tanggal rilis produk mundur ke akhir bulan karena pengujian belum selesai.", "Tim QA menemukan tiga bug kritis pada pembayaran."]),
    ("Rapat pemasaran", &["Kampanye media sosial menghasilkan dua kali lipat pengunjung situs.", "Anggaran iklan akan ditambah untuk bulan depan."]),
];

const QUESTIONS: [(&str, &str); 4] = [
    ("Kenapa pengeluaran komputasi awan membengkak?", "Rapat infrastruktur"),
    ("Siapa yang akan kita tambahkan ke tim teknis?", "Rapat rekrutmen"),
    ("Mengapa jadwal peluncuran tertunda?", "Rapat peluncuran"),
    ("Bagaimana hasil promosi online?", "Rapat pemasaran"),
];

#[test]
#[ignore = "needs an embedding provider"]
fn questions_find_the_meeting_that_says_it_in_other_words() {
    let key = std::env::var("EMBEDDING_API_KEY").expect("EMBEDDING_API_KEY is not set");
    let base_url =
        std::env::var("EMBEDDING_BASE_URL").unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta/openai".into());
    let model = std::env::var("EMBEDDING_MODEL").ok().or_else(|| default_model(&base_url).map(str::to_owned)).expect("EMBEDDING_MODEL is not set");
    let embedder = OpenAiEmbedder::new(&base_url, &key, &model).unwrap();

    let path = std::env::temp_dir().join(format!("notetaker-live-embeddings-{}.db", std::process::id()));
    let mut store = Store::open(&path).unwrap();
    for (position, (title, lines)) in MEETINGS.iter().enumerate() {
        let id = store.create_meeting(title, position as i64).unwrap();
        let segments = lines
            .iter()
            .enumerate()
            .map(|(i, text)| TranscriptSegment { source: Source::System, start_secs: i as f64 * 10.0, end_secs: i as f64 * 10.0 + 8.0, text: (*text).into(), speaker: None })
            .collect();
        store.save(id, &NotesSnapshot { segments, ..NotesSnapshot::default() }).unwrap();
    }
    let db = Mutex::new(store);

    assert_eq!(index_pending(&db, &embedder).unwrap(), 4);
    let mut failures = Vec::new();
    for (question, expected) in QUESTIONS {
        let hits = search(&db, &embedder, question, 5).unwrap();
        let shown: Vec<String> = hits.iter().map(|hit| format!("{} {:.3}", hit.meeting.title, hit.score)).collect();
        println!("{question} -> {shown:?}");
        if hits.first().map(|hit| hit.meeting.title.as_str()) != Some(expected) {
            failures.push(question);
        }
    }
    drop(db);
    let _ = std::fs::remove_file(&path);
    assert!(failures.is_empty(), "wrong meeting for: {failures:?}");
}
