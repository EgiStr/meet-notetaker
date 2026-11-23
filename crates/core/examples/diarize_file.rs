//! Usage: cargo run --release -p notetaker-core --example diarize_file -- <recording>
use notetaker_core::speakers::Diarizer;

fn main() {
    let path = std::env::args().nth(1).expect("usage: diarize_file <recording>");
    let started = std::time::Instant::now();
    let turns = notetaker_core::polyvoice_diarizer::PolyvoiceDiarizer.diarize(std::path::Path::new(&path)).unwrap();
    let mut voices: std::collections::BTreeMap<u32, f64> = Default::default();
    for t in &turns { *voices.entry(t.speaker).or_default() += t.end_secs - t.start_secs; }
    println!("{} turns in {:?}; speech seconds by voice: {voices:?}", turns.len(), started.elapsed());
}
