//! The minutes laid out like a formal "Minutes of Meeting" document (HTML that prints to PDF), and
//! the raw exports of a meeting: plain transcript text and JSON.

use crate::export::{clock, format_date};
use crate::notes::NotesSnapshot;
use crate::store::MeetingSummary;
use crate::transcript::Source;

/// The raw transcript as plain text, one line per segment with its time and speaker.
pub fn transcript_text(summary: &MeetingSummary, notes: &NotesSnapshot, utc_offset_secs: i64) -> String {
    let mut out = format!("{}\n{}\n\n", summary.title, format_date(summary.started_at, utc_offset_secs));
    for segment in &notes.segments {
        out.push_str(&format!("[{}] {}: {}\n", clock(segment.start_secs), notes.names.label(segment), segment.text));
    }
    out
}

/// Everything captured, as JSON for other tools: segments with their exact times and speakers.
pub fn raw_json(summary: &MeetingSummary, notes: &NotesSnapshot, utc_offset_secs: i64) -> String {
    let segments: Vec<serde_json::Value> = notes
        .segments
        .iter()
        .map(|segment| {
            serde_json::json!({
                "start_secs": segment.start_secs,
                "end_secs": segment.end_secs,
                "source": if segment.source == Source::Microphone { "microphone" } else { "system" },
                "speaker": notes.names.label(segment),
                "text": segment.text,
            })
        })
        .collect();
    let document = serde_json::json!({
        "title": summary.title,
        "started_at": summary.started_at,
        "date": format_date(summary.started_at, utc_offset_secs),
        "duration_secs": summary.duration_secs,
        "minutes": notes.mom,
        "notes": notes.user_notes,
        "segments": segments,
    });
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Escapes text and turns `**bold**` and `*italic*` into tags.
fn inline(text: &str) -> String {
    let escaped = escape(text);
    let mut out = String::new();
    let mut rest = escaped.as_str();
    while let Some(start) = rest.find('*') {
        let double = rest[start..].starts_with("**");
        let marker = if double { "**" } else { "*" };
        let after = &rest[start + marker.len()..];
        match after.find(marker) {
            Some(end) if end > 0 => {
                let tag = if double { "strong" } else { "em" };
                out.push_str(&rest[..start]);
                out.push_str(&format!("<{tag}>{}</{tag}>", &after[..end]));
                rest = &after[end + marker.len()..];
            }
            _ => {
                out.push_str(&rest[..start + marker.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn table_cells(line: &str) -> Vec<&str> {
    line.trim().trim_matches('|').split('|').map(str::trim).collect()
}

fn is_table_rule(line: &str) -> bool {
    let cells = table_cells(line);
    cells.iter().all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

fn close_list(out: &mut String, in_list: &mut bool) {
    if *in_list {
        out.push_str("</ul>\n");
        *in_list = false;
    }
}

/// The Markdown of the minutes as HTML: headings, bullets, tables, `>` call-out boxes, paragraphs.
pub fn markdown_to_html(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = String::new();
    let mut in_list = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        index += 1;
        if line.is_empty() {
            close_list(&mut out, &mut in_list);
        } else if let Some(heading) = line.strip_prefix("### ") {
            close_list(&mut out, &mut in_list);
            out.push_str(&format!("<h3>{}</h3>\n", inline(heading)));
        } else if let Some(heading) = line.strip_prefix("## ").or_else(|| line.strip_prefix("# ")) {
            close_list(&mut out, &mut in_list);
            out.push_str(&format!("<h2>{}</h2>\n", inline(heading)));
        } else if line.starts_with('|') {
            close_list(&mut out, &mut in_list);
            let mut rows = vec![line];
            while index < lines.len() && lines[index].trim().starts_with('|') {
                rows.push(lines[index].trim());
                index += 1;
            }
            out.push_str("<table>\n");
            let mut header = true;
            for row in rows {
                if is_table_rule(row) {
                    continue;
                }
                let tag = if header { "th" } else { "td" };
                out.push_str("<tr>");
                for cell in table_cells(row) {
                    out.push_str(&format!("<{tag}>{}</{tag}>", inline(cell)));
                }
                out.push_str("</tr>\n");
                header = false;
            }
            out.push_str("</table>\n");
        } else if let Some(quote) = line.strip_prefix('>') {
            close_list(&mut out, &mut in_list);
            out.push_str(&format!("<div class=\"callout\">{}</div>\n", inline(quote.trim())));
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if !in_list {
                out.push_str("<ul>\n");
                in_list = true;
            }
            out.push_str(&format!("<li>{}</li>\n", inline(item)));
        } else {
            close_list(&mut out, &mut in_list);
            out.push_str(&format!("<p>{}</p>\n", inline(line)));
        }
    }
    close_list(&mut out, &mut in_list);
    out
}

const REPORT_CSS: &str = "\
@page { size: A4; margin: 20mm 18mm 22mm; @bottom-left { content: \"Minutes of Meeting\"; font: 8pt Arial; color: #667; } @bottom-right { content: \"Page \" counter(page); font: 8pt Arial; color: #667; } }
body { font: 10pt/1.5 Arial, Helvetica, sans-serif; color: #1a2233; margin: 0 auto; max-width: 780px; padding: 24px; }
.kicker { font-size: 8.5pt; letter-spacing: .04em; color: #667; text-transform: uppercase; }
h1 { font-size: 22pt; margin: 4px 0 6px; color: #14233f; }
.meta { color: #667; font-size: 10pt; border-bottom: 1px solid #14233f; padding-bottom: 10px; margin-bottom: 12px; }
h2 { font-size: 15pt; color: #14233f; margin: 22px 0 8px; break-after: avoid; }
h3 { font-size: 11.5pt; color: #2c5d9b; margin: 14px 0 6px; break-after: avoid; }
ul { margin: 4px 0 8px; padding-left: 18px; } li { margin: 0 0 6px; }
.callout, .source { background: #edf2f7; border: 1px solid #d6dfe9; border-radius: 3px; padding: 8px 10px; margin: 10px 0; }
table { border-collapse: collapse; width: 100%; margin: 8px 0 12px; font-size: 9pt; }
th { background: #14233f; color: #fff; text-align: left; padding: 6px 8px; }
td { padding: 6px 8px; border-bottom: 1px solid #d6dfe9; vertical-align: top; } tr:nth-child(even) td { background: #f6f8fb; }
.footer { color: #778; font-size: 8pt; border-top: 1px solid #d6dfe9; margin-top: 18px; padding-top: 6px; }
.transcript p { margin: 2px 0; font-size: 9pt; } .transcript .t { color: #778; }
@media print { body { padding: 0; max-width: none; } -webkit-print-color-adjust: exact; print-color-adjust: exact; }";

/// The minutes as a document in the layout of a formal "Minutes of Meeting" file, to print to PDF.
pub fn report_html(summary: &MeetingSummary, notes: &NotesSnapshot, utc_offset_secs: i64) -> String {
    let minutes = (summary.duration_secs / 60.0).round().max(1.0);
    let mut labels: Vec<String> = Vec::new();
    for segment in &notes.segments {
        let label = notes.names.label(segment);
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    let speakers = if labels.is_empty() { "belum ada".to_owned() } else { labels.join(", ") };
    let unseparated = labels.iter().any(|label| label == "Lainnya");
    let voices = if unseparated {
        "pemisahan suara tidak tersedia untuk sebagian ucapan, yang ditandai \"Lainnya\""
    } else {
        "Pembicara 1, 2, 3 … hasil pemisahan suara"
    };

    let mut body = String::new();
    body.push_str("<div class=\"kicker\">Minutes of Meeting</div>\n");
    body.push_str(&format!("<h1>{}</h1>\n", escape(&summary.title)));
    body.push_str(&format!(
        "<div class=\"meta\">{} | Durasi rekaman ±{minutes} menit | Pembicara: {}</div>\n",
        format_date(summary.started_at, utc_offset_secs),
        escape(&speakers)
    ));
    body.push_str("<p><strong>Tujuan dokumen.</strong> Merangkum pembahasan, keputusan, dan tindak lanjut rapat ini sebagai arsip kerja.</p>\n");
    body.push_str(&format!(
        "<div class=\"source\"><strong>Catatan sumber.</strong> Notulen disusun otomatis dari transkrip pengenalan suara, sehingga nama, istilah teknis, dan angka dapat keliru. \"Saya\" adalah pemilik rekaman (mikrofon); peserta lain diberi nomor ({voices}). Nama pembicara dan penanggung jawab perlu dikonfirmasi ke peserta atau rekaman.</div>\n"
    ));
    body.push_str(&markdown_to_html(&notes.mom));
    if !notes.user_notes.trim().is_empty() {
        body.push_str("<h2>Catatan Saya</h2>\n");
        body.push_str(&markdown_to_html(&notes.user_notes));
    }
    body.push_str("<h2>Lampiran: Transkrip</h2>\n<div class=\"transcript\">\n");
    for segment in &notes.segments {
        body.push_str(&format!(
            "<p><span class=\"t\">[{}]</span> <strong>{}</strong>: {}</p>\n",
            clock(segment.start_secs),
            escape(&notes.names.label(segment)),
            escape(&segment.text)
        ));
    }
    body.push_str("</div>\n<div class=\"footer\">Dokumen ini adalah notulen kerja yang disusun dari transkrip otomatis. Mohon validasi keputusan, penanggung jawab, istilah, dan target waktu kepada peserta rapat sebelum digunakan sebagai baseline resmi.</div>\n");
    format!(
        "<!doctype html>\n<html lang=\"id\"><head><meta charset=\"utf-8\"><title>{}</title><style>{REPORT_CSS}</style></head><body>\n{body}</body></html>\n",
        escape(&summary.title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::TranscriptSegment;

    #[test]
    fn the_minutes_become_html_with_tables_callouts_and_escaped_text() {
        let html = markdown_to_html(
            "## 1. Ringkasan\n- **Rilis** <mundur>\n\n> **Belum diputuskan:** harga\n\n| # | Tugas |\n| --- | --- |\n| 1 | Kirim laporan |\n\n### 2.1 Topik\nTeks biasa",
        );
        for expected in [
            "<h2>1. Ringkasan</h2>",
            "<li><strong>Rilis</strong> &lt;mundur&gt;</li>",
            "<div class=\"callout\"><strong>Belum diputuskan:</strong> harga</div>",
            "<th>#</th><th>Tugas</th>",
            "<td>1</td><td>Kirim laporan</td>",
            "<h3>2.1 Topik</h3>",
            "<p>Teks biasa</p>",
        ] {
            assert!(html.contains(expected), "missing {expected:?} in\n{html}");
        }
        assert!(!html.contains("---"));
    }

    #[test]
    fn the_report_and_the_raw_exports_carry_the_speakers() {
        let summary = MeetingSummary { id: 1, title: "Rapat <A>".into(), started_at: 0, duration_secs: 3000.0 };
        let notes = NotesSnapshot {
            segments: vec![TranscriptSegment {
                source: Source::System,
                start_secs: 5.0,
                end_secs: 9.0,
                text: "Setuju.".into(),
                speaker: Some(2),
            }],
            mom: "## 1. Executive Summary\n- **Anggaran:** disetujui".into(),
            ..NotesSnapshot::default()
        };
        let html = report_html(&summary, &notes, 0);
        assert!(html.contains("Minutes of Meeting") && html.contains("Rapat &lt;A&gt;") && html.contains("±50 menit"));
        assert!(html.contains("Pembicara: Pembicara 2") && html.contains("<strong>Pembicara 2</strong>: Setuju."));
        assert_eq!(transcript_text(&summary, &notes, 0).lines().nth(3), Some("[00:05] Pembicara 2: Setuju."));
        let json: serde_json::Value = serde_json::from_str(&raw_json(&summary, &notes, 0)).unwrap();
        assert_eq!(json["segments"][0]["speaker"], "Pembicara 2");
        assert_eq!(json["segments"][0]["source"], "system");
    }
}
