// Derived from herdr src/app/tab_bar_status.rs and platform/unix_common.rs (Apache-2.0).
use super::*;
use crate::config::TabBarRightEntryConfig;

impl App {
    pub(super) fn update_tab_status(&mut self, now: Instant) {
        let mut timed = false;
        let hostname = self
            .view
            .core
            .as_ref()
            .map(|core| core.name.as_str())
            .unwrap_or("");
        let segments = self
            .shell
            .tab_bar_right
            .iter()
            .take(16)
            .filter_map(|entry| {
                let text = match entry {
                    TabBarRightEntryConfig::Text { text } => {
                        text.chars().filter(|ch| !ch.is_control()).collect()
                    }
                    TabBarRightEntryConfig::Hostname => sanitize_status_text(hostname)?,
                    TabBarRightEntryConfig::Datetime { format } => {
                        timed = true;
                        let format =
                            crate::config::tab_bar::parse_tab_bar_datetime_format(format).ok()?;
                        sanitize_status_text(&local_datetime()?.format(&format).ok()?)?
                    }
                    // M2 has no zoom; M4 owns configured shell commands.
                    TabBarRightEntryConfig::Zoom | TabBarRightEntryConfig::Command { .. } => {
                        return None;
                    }
                };
                (!text.is_empty()).then_some(crate::chrome::snapshot::ClientShellTabStatusSegment {
                    text,
                    accent: false,
                })
            })
            .collect::<Vec<_>>();
        let separator = self
            .shell
            .tab_bar_right_separator
            .chars()
            .filter(|ch| !ch.is_control())
            .collect::<String>();
        self.projection.snapshot.tab_bar_right = segments.clone();
        self.projection.snapshot.tab_bar_right_separator = separator.clone();
        for endpoint in &mut self.projection.endpoints {
            if endpoint.endpoint_id == self.projection.active_endpoint_id
                && let Some(snapshot) = &mut endpoint.snapshot
            {
                snapshot.tab_bar_right = segments.clone();
                snapshot.tab_bar_right_separator = separator.clone();
            }
        }
        self.next_tab_status = timed.then(|| now + std::time::Duration::from_secs(1));
    }
}

fn local_datetime() -> Option<time::PrimitiveDateTime> {
    let mut timestamp: libc::time_t = 0;
    if unsafe { libc::time(&mut timestamp) } == -1 {
        return None;
    }
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return None;
    }
    let month = time::Month::try_from(u8::try_from(local.tm_mon + 1).ok()?).ok()?;
    let date = time::Date::from_calendar_date(
        local.tm_year + 1900,
        month,
        u8::try_from(local.tm_mday).ok()?,
    )
    .ok()?;
    let time = time::Time::from_hms(
        u8::try_from(local.tm_hour).ok()?,
        u8::try_from(local.tm_min).ok()?,
        u8::try_from(local.tm_sec).ok()?,
    )
    .ok()?;
    Some(time::PrimitiveDateTime::new(date, time))
}

fn sanitize_status_text(value: &str) -> Option<String> {
    let value: String = value
        .trim()
        .chars()
        .filter(|character| !character.is_control() && !is_unicode_format_control(*character))
        .take(80)
        .collect();
    (!value.is_empty()).then_some(value)
}

fn is_unicode_format_control(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061c}'
            | '\u{06dd}'
            | '\u{070f}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08e2}'
            | '\u{17b4}'..='\u{17b5}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}
