//! Tells the voices of a recording apart.
//!
//! cargo run --release -p notetaker-core --example diarize_file -- <recording>
//!     prints how long each voice speaks.
//! cargo run --release -p notetaker-core --example diarize_file -- <recording> <meetings.db> <meeting id>
//!     also labels the system lines of that meeting and saves it. Close the app first.
use std::path::Path;

use notetaker_core::speakers::{assign_speakers, Diarizer};
use notetaker_core::store::Store;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: diarize_file <recording> [<meetings.db> <meeting id>]");
    let started = std::time::Instant::now();
    let turns = notetaker_core::polyvoice_diarizer::PolyvoiceDiarizer.diarize(Path::new(path)).unwrap();
    let mut voices: std::collections::BTreeMap<u32, f64> = Default::default();
    for turn in &turns {
        *voices.entry(turn.speaker).or_default() += turn.end_secs - turn.start_secs;
    }
    println!("{} turns in {:?}; speech seconds by voice: {voices:?}", turns.len(), started.elapsed());

    if let [_, database, id] = args.as_slice() {
        let id: i64 = id.parse().expect("meeting id");
        let mut store = Store::open(Path::new(database)).unwrap();
        let (_, mut snapshot) = store.load(id).unwrap().expect("meeting not found");
        assign_speakers(&mut snapshot.segments, &turns);
        store.save(id, &snapshot).unwrap();
        let mut counts: std::collections::BTreeMap<Option<u32>, usize> = Default::default();
        for segment in &snapshot.segments {
            *counts.entry(segment.speaker).or_default() += 1;
        }
        println!("saved; segments by speaker: {counts:?}");
    }
}
