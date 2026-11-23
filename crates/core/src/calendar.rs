//! The calendar, read from an iCalendar (.ics) feed: which meeting is on right now, and who was
//! invited. Google Calendar ("secret address in iCal format") and Outlook publish such a feed, so
//! no sign-in is needed.
//!
//! Not supported: time zones other than UTC and the user's own (a zoned time is read as local
//! time), exceptions to a recurring meeting, and rules other than daily and weekly.

use std::time::Duration;

/// A meeting may be started this early, and still be taken for the calendar entry.
const EARLY_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub title: String,
    /// Seconds since the Unix epoch.
    pub start: i64,
    pub end: i64,
    pub attendees: Vec<String>,
    rule: Option<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
struct Rule {
    weekly: bool,
    interval: i64,
    /// Monday is 0. Empty means the weekday of the first occurrence.
    weekdays: Vec<i64>,
    until: Option<i64>,
}

/// The entries of an .ics text. `utc_offset_secs` is the user's, for times given without a zone.
pub fn parse(text: &str, utc_offset_secs: i64) -> Vec<Event> {
    let mut events = Vec::new();
    let mut current: Option<Event> = None;
    for line in unfold(text) {
        let upper = line.to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            current = Some(Event { title: String::new(), start: 0, end: 0, attendees: Vec::new(), rule: None });
            continue;
        }
        if upper == "END:VEVENT" {
            if let Some(event) = current.take().filter(|event| event.start > 0) {
                let end = if event.end > event.start { event.end } else { event.start + 3600 };
                events.push(Event { end, ..event });
            }
            continue;
        }
        let (Some(event), Some((name_and_params, value))) = (current.as_mut(), line.split_once(':')) else { continue };
        let mut parts = name_and_params.split(';');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let params: Vec<&str> = parts.collect();
        match name.as_str() {
            "SUMMARY" => event.title = unescape(value),
            "DTSTART" => event.start = date_time(value, utc_offset_secs).unwrap_or(0),
            "DTEND" => event.end = date_time(value, utc_offset_secs).unwrap_or(0),
            "ATTENDEE" | "ORGANIZER" => {
                if params.iter().any(|param| param.eq_ignore_ascii_case("CUTYPE=RESOURCE")) {
                    continue;
                }
                let common = params.iter().find_map(|param| {
                    let (key, value) = param.split_once('=')?;
                    key.eq_ignore_ascii_case("CN").then(|| value.trim_matches('"').to_owned())
                });
                let from_address = value.rsplit(':').next().and_then(|address| address.split('@').next()).map(str::to_owned);
                if let Some(person) = common.or(from_address).map(|name| unescape(&name)).filter(|name| !name.trim().is_empty()) {
                    if !event.attendees.contains(&person) {
                        event.attendees.push(person);
                    }
                }
            }
            "RRULE" => event.rule = rule(value, utc_offset_secs),
            _ => {}
        }
    }
    events
}

/// The entry that is on at `now`, or about to start; the one that starts closest to `now` wins.
pub fn current(events: &[Event], now: i64, utc_offset_secs: i64) -> Option<Event> {
    events
        .iter()
        .filter_map(|event| occurrence_near(event, now, utc_offset_secs))
        .filter(|event| event.start - EARLY_SECS <= now && now <= event.end)
        .min_by_key(|event| (event.start - now).abs())
}

/// `event` moved to the day of `now` when it recurs and that day is one of its days.
fn occurrence_near(event: &Event, now: i64, utc_offset_secs: i64) -> Option<Event> {
    let Some(rule) = &event.rule else { return Some(event.clone()) };
    let day = |unix: i64| (unix + utc_offset_secs).div_euclid(86_400);
    let (first_day, today) = (day(event.start), day(now));
    if today < first_day || rule.until.is_some_and(|until| now > until + 86_400) {
        return None;
    }
    let matches = if rule.weekly {
        let weekday = |days: i64| (days + 3).rem_euclid(7);
        let on_day = if rule.weekdays.is_empty() { weekday(first_day) == weekday(today) } else { rule.weekdays.contains(&weekday(today)) };
        // Weeks are counted from the Monday of the first occurrence's week.
        let weeks = (today - weekday(today) - (first_day - weekday(first_day))) / 7;
        on_day && weeks % rule.interval == 0
    } else {
        (today - first_day) % rule.interval == 0
    };
    matches.then(|| {
        let shift = (today - first_day) * 86_400;
        Event { start: event.start + shift, end: event.end + shift, rule: None, ..event.clone() }
    })
}

fn rule(value: &str, utc_offset_secs: i64) -> Option<Rule> {
    let mut rule = Rule { weekly: false, interval: 1, weekdays: Vec::new(), until: None };
    let mut known = false;
    for part in value.split(';') {
        let (key, value) = part.split_once('=')?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => match value.to_ascii_uppercase().as_str() {
                "DAILY" => known = true,
                "WEEKLY" => {
                    rule.weekly = true;
                    known = true;
                }
                _ => return None,
            },
            "INTERVAL" => rule.interval = value.parse::<i64>().ok().filter(|n| *n > 0)?,
            "BYDAY" => {
                rule.weekdays = value
                    .split(',')
                    .filter_map(|day| ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|name| day.ends_with(name)))
                    .map(|index| index as i64)
                    .collect();
            }
            "UNTIL" => rule.until = date_time(value, utc_offset_secs),
            _ => {}
        }
    }
    known.then_some(rule)
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        match (raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.trim_end().to_owned()),
        }
    }
    lines
}

fn unescape(text: &str) -> String {
    text.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\").trim().to_owned()
}

/// `20261123T090000Z` is UTC; without the Z it is the user's local time. A date alone (an
/// all-day entry) has no time and is skipped.
fn date_time(value: &str, utc_offset_secs: i64) -> Option<i64> {
    let value = value.trim();
    let (utc, digits) = (value.ends_with('Z'), value.trim_end_matches('Z'));
    let (date, time) = digits.split_once('T')?;
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let number = |text: &str| text.parse::<i64>().ok();
    let (year, month, day) = (number(&date[0..4])?, number(&date[4..6])?, number(&date[6..8])?);
    let (hour, minute) = (number(&time[0..2])?, number(&time[2..4])?);
    let second = time.get(4..6).and_then(number).unwrap_or(0);
    let local = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(if utc { local } else { local - utc_offset_secs })
}

/// Days from 1970-01-01 to a civil date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Reads a feed from a web address or a file on this computer.
pub fn fetch(source: &str) -> Result<String, String> {
    let source = source.trim();
    // Calendar apps hand out "webcal://" addresses for the same feed over https.
    let source = source.strip_prefix("webcal://").map_or_else(|| source.to_owned(), |rest| format!("https://{rest}"));
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
        let response = client.get(&source).send().map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("kalender menolak permintaan ({})", response.status()));
        }
        response.text().map_err(|e| e.to_string())
    } else {
        std::fs::read_to_string(&source).map_err(|error| format!("{source}: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIB: i64 = 7 * 3600;

    fn feed(extra: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Sinkronisasi\\, mingguan\r\nDTSTART:20261123T020000Z\r\nDTEND:20261123T030000Z\r\n\
ATTENDEE;CN=Budi Santoso;PARTSTAT=ACCEPTED:mailto:budi@x.test\r\nATTENDEE;CN=\"Ani\":mailto:ani@x.test\r\n\
ATTENDEE;CUTYPE=RESOURCE;CN=Ruang 3:mailto:room@x.test\r\nORGANIZER:mailto:citra@x.test\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n"
        )
    }

    #[test]
    fn an_entry_has_title_time_and_invited_people() {
        let events = parse(&feed(""), WIB);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Sinkronisasi, mingguan");
        assert_eq!(events[0].start, 1_795_399_200); // 2026-11-23 02:00 UTC
        assert_eq!(events[0].end - events[0].start, 3600);
        assert_eq!(events[0].attendees, ["Budi Santoso", "Ani", "citra"]);
    }

    #[test]
    fn long_lines_are_unfolded_and_all_day_entries_skipped() {
        let text = "BEGIN:VEVENT\r\nSUMMARY:Rapat sangat\r\n  panjang\r\nDTSTART:20261123T090000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:Libur\r\nDTSTART;VALUE=DATE:20261123\r\nEND:VEVENT\r\n";
        let events = parse(text, WIB);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Rapat sangat panjang");
        assert_eq!(events[0].start, 1_795_399_200); // 09:00 local (UTC+7) is 02:00 UTC
        assert_eq!(events[0].end, events[0].start + 3600);
    }

    #[test]
    fn the_entry_that_is_on_now_or_starts_soon_is_found() {
        let events = parse(&feed(""), WIB);
        let start = events[0].start;
        assert!(current(&events, start - 600, WIB).is_some());
        assert!(current(&events, start + 1800, WIB).is_some());
        assert!(current(&events, start - 3600, WIB).is_none());
        assert!(current(&events, start + 7200, WIB).is_none());
    }

    #[test]
    fn a_weekly_entry_recurs_on_its_days_and_stops_at_until() {
        let events = parse(&feed("RRULE:FREQ=WEEKLY;BYDAY=MO,TH;UNTIL=20261231T000000Z\r\n"), WIB);
        let start = events[0].start; // a Monday
        let day = 86_400;
        assert!(current(&events, start + 3 * day + 600, WIB).is_some()); // Thursday
        assert!(current(&events, start + 7 * day + 600, WIB).is_some()); // next Monday
        assert!(current(&events, start + 2 * day + 600, WIB).is_none()); // Wednesday
        assert!(current(&events, start + 60 * day + 600, WIB).is_none()); // after UNTIL
        assert!(current(&events, start - 7 * day + 600, WIB).is_none()); // before it began
        let fortnightly = parse(&feed("RRULE:FREQ=WEEKLY;INTERVAL=2\r\n"), WIB);
        assert!(current(&fortnightly, start + 7 * day + 600, WIB).is_none());
        assert!(current(&fortnightly, start + 14 * day + 600, WIB).is_some());
        let daily = parse(&feed("RRULE:FREQ=DAILY\r\n"), WIB);
        assert_eq!(current(&daily, start + 5 * day + 600, WIB).map(|e| e.start), Some(start + 5 * day));
    }

    #[test]
    fn unsupported_rules_are_treated_as_one_off() {
        let events = parse(&feed("RRULE:FREQ=MONTHLY\r\n"), WIB);
        assert!(current(&events, events[0].start + 30 * 86_400, WIB).is_none());
        assert!(current(&events, events[0].start + 600, WIB).is_some());
    }
}
