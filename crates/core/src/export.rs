//! A meeting as one Markdown document that can be kept, shared or opened elsewhere.

use crate::notes::NotesSnapshot;
use crate::store::MeetingSummary;

/// `utc_offset_secs` is the local time zone, for the date shown.
pub fn markdown(summary: &MeetingSummary, notes: &NotesSnapshot, utc_offset_secs: i64) -> String {
    let mut out = format!(
        "# {}\n\n*{} · {} menit*\n\n",
        summary.title,
        format_date(summary.started_at, utc_offset_secs),
        (summary.duration_secs / 60.0).round().max(1.0)
    );

    out.push_str(&section_or_text(&notes.mom, "## Notulen\n\n(belum ada)"));
    out.push_str("\n\n## Catatan Saya\n\n");
    out.push_str(text_or_none(&notes.user_notes));

    if !notes.bookmarks.is_empty() {
        out.push_str("\n\n## Penanda\n\n");
        for bookmark in &notes.bookmarks {
            out.push_str(&format!("- [{}] {}\n", clock(bookmark.at_secs), bookmark.label));
        }
    }

    out.truncate(out.trim_end().len());
    out.push_str("\n\n## Transkrip\n\n");
    if notes.segments.is_empty() {
        out.push_str("(belum ada)\n");
    }
    for segment in &notes.segments {
        out.push_str(&format!("- [{}] **{}**: {}\n", clock(segment.start_secs), notes.names.label(segment), segment.text));
    }

    if !notes.chat.is_empty() {
        out.push_str("\n## Tanya Jawab\n\n");
        for exchange in &notes.chat {
            out.push_str(&format!("**{}**\n\n{}\n\n", exchange.question, exchange.answer));
        }
    }
    out
}

/// `YYYY-MM-DD HH:MM` for a Unix time in the time zone `utc_offset_secs` east of UTC.
pub fn format_date(unix_secs: i64, utc_offset_secs: i64) -> String {
    let local = unix_secs + utc_offset_secs;
    let (days, secs_of_day) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}", secs_of_day / 3600, secs_of_day % 3600 / 60)
}

fn clock(secs: f64) -> String {
    let whole = secs.max(0.0) as u64;
    format!("{:02}:{:02}", whole / 60, whole % 60)
}

fn section_or_text(text: &str, none: &str) -> String {
    match text.trim() {
        "" => none.to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

fn text_or_none(text: &str) -> &str {
    match text.trim() {
        "" => "(belum ada)",
        trimmed => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Bookmark, Exchange};
    use crate::transcript::{Source, TranscriptSegment};

    #[test]
    fn dates_follow_the_time_zone() {
        assert_eq!(format_date(0, 0), "1970-01-01 00:00");
        // 2024-02-29 23:30 UTC is already the next day in Jakarta (UTC+7).
        assert_eq!(format_date(1_709_249_400, 0), "2024-02-29 23:30");
        assert_eq!(format_date(1_709_249_400, 7 * 3600), "2024-03-01 06:30");
        assert_eq!(format_date(-86_400, 0), "1969-12-31 00:00");
    }

    #[test]
    fn the_document_holds_everything_that_was_kept() {
        let summary = MeetingSummary { id: 1, title: "Rapat anggaran".into(), started_at: 0, duration_secs: 150.0 };
        let notes = NotesSnapshot {
            segments: vec![TranscriptSegment {
                source: Source::System,
                start_secs: 65.0,
                end_secs: 70.0,
                text: "Disetujui.".into(),
                speaker: Some(1),
            }],
            mom: "## Ringkasan\n- Anggaran".into(),
            user_notes: "fokus anggaran".into(),
            names: [(1, "Budi".to_string())].into_iter().collect(),
            chat: vec![Exchange { question: "Siapa?".into(), answer: "Budi.".into() }],
            bookmarks: vec![Bookmark { at_secs: 65.0, label: "keputusan".into() }],
            ..NotesSnapshot::default()
        };

        let document = markdown(&summary, &notes, 0);
        assert!(document.starts_with("# Rapat anggaran\n\n*1970-01-01 00:00 · 3 menit*"));
        for expected in [
            "## Ringkasan\n- Anggaran",
            "## Catatan Saya\n\nfokus anggaran",
            "- [01:05] keputusan",
            "- [01:05] **Budi**: Disetujui.",
            "**Siapa?**\n\nBudi.",
        ] {
            assert!(document.contains(expected), "missing {expected:?} in\n{document}");
        }
    }

    #[test]
    fn an_empty_meeting_still_exports() {
        let summary = MeetingSummary { id: 1, title: "Kosong".into(), started_at: 0, duration_secs: 0.0 };
        let document = markdown(&summary, &NotesSnapshot::default(), 0);
        assert!(document.contains("## Notulen\n\n(belum ada)"));
        assert!(!document.contains("## Penanda") && !document.contains("## Tanya Jawab"));
    }
}
