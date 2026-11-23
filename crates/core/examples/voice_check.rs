//! Builds the voiceprints of a stored meeting and prints them, to check the voice memory:
//! cargo run --release -p notetaker-core --example voice_check -- <meetings.db> <meeting id> <system recording>
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [database, id, recording] = args.as_slice() else { panic!("usage: voice_check <meetings.db> <meeting id> <recording>") };
    let started = std::time::Instant::now();
    let embedder = notetaker_core::voiceprint::Resnet::load().expect("model");
    let store = notetaker_core::store::Store::open(Path::new(database)).unwrap();
    let (_, snapshot) = store.load(id.parse().unwrap()).unwrap().expect("meeting");
    let samples = notetaker_core::audio_file::read_samples(Path::new(recording)).unwrap();
    println!("loaded in {:?}", started.elapsed());
    let prints = notetaker_core::voiceprint::speaker_prints(&embedder, &samples, &snapshot.segments, notetaker_core::audio::SAMPLE_RATE);
    println!("{} prints in {:?}", prints.len(), started.elapsed());
    let keys: Vec<_> = prints.keys().collect();
    for a in &keys {
        let row: Vec<String> = keys.iter().map(|b| format!("{:.2}", notetaker_core::embed::cosine(&prints[*a], &prints[*b]))).collect();
        println!("voice {a}: {}", row.join(" "));
    }
}
