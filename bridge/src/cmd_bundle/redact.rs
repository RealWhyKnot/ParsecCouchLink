//! Bundle text redaction. Keep diagnostic structure, counters, lengths,
//! timings, and failure codes while stripping values that identify Wi-Fi
//! networks or credentials.

const SENSITIVE_KEYS: &[&str] = &[
    "ssid",
    "password",
    "pass",
    "wifi_password",
    "token",
    "secret",
    "authorization",
    "api_key",
    "apikey",
];

pub(super) fn redact_bundle_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for segment in input.split_inclusive('\n') {
        let (line, suffix) = segment
            .strip_suffix('\n')
            .map(|s| {
                (
                    s.strip_suffix('\r').unwrap_or(s),
                    if s.ends_with('\r') { "\r\n" } else { "\n" },
                )
            })
            .unwrap_or((segment, ""));
        out.push_str(&redact_line(line));
        out.push_str(suffix);
    }
    if input.is_empty() {
        return out;
    }
    if !input.ends_with('\n') && out.is_empty() {
        return redact_line(input);
    }
    out
}

fn redact_line(line: &str) -> String {
    let line = redact_wifi_join_line(line);
    redact_key_values(&line)
}

fn redact_wifi_join_line(line: &str) -> String {
    const PREFIX: &str = "wifi: starting join to ";
    const LEN_MARKER: &str = " (ssid_len=";
    let Some(start) = line.find(PREFIX) else {
        return line.to_string();
    };
    let value_start = start + PREFIX.len();
    let Some(rel_end) = line[value_start..].find(LEN_MARKER) else {
        return line.to_string();
    };
    let value_end = value_start + rel_end;
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..value_start]);
    out.push_str("<redacted>");
    out.push_str(&line[value_end..]);
    out
}

fn redact_key_values(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < line.len() {
        let Some((key_start, value_start)) = find_sensitive_value(line, index) else {
            out.push_str(&line[index..]);
            break;
        };
        out.push_str(&line[index..value_start]);
        let value_end = find_value_end(line, value_start);
        if value_end > value_start {
            out.push_str("<redacted>");
        }
        index = value_end;
        if key_start == index {
            break;
        }
    }
    out
}

fn find_sensitive_value(line: &str, offset: usize) -> Option<(usize, usize)> {
    let lower = line.to_ascii_lowercase();
    let haystack = &lower[offset..];
    let mut best: Option<(usize, usize)> = None;
    for key in SENSITIVE_KEYS {
        let mut search_from = 0usize;
        while let Some(pos) = haystack[search_from..].find(key) {
            let key_start = offset + search_from + pos;
            if !is_key_boundary(line, key_start, key.len()) {
                search_from += pos + key.len();
                continue;
            }
            let mut sep = key_start + key.len();
            while sep < line.len() && line.as_bytes()[sep].is_ascii_whitespace() {
                sep += 1;
            }
            if sep >= line.len() || (line.as_bytes()[sep] != b'=' && line.as_bytes()[sep] != b':') {
                search_from += pos + key.len();
                continue;
            }
            sep += 1;
            while sep < line.len() && line.as_bytes()[sep].is_ascii_whitespace() {
                sep += 1;
            }
            if best
                .map(|(best_start, _)| key_start < best_start)
                .unwrap_or(true)
            {
                best = Some((key_start, sep));
            }
            break;
        }
    }
    best
}

fn is_key_boundary(line: &str, key_start: usize, key_len: usize) -> bool {
    let before_ok = key_start == 0
        || !line.as_bytes()[key_start - 1].is_ascii_alphanumeric()
            && line.as_bytes()[key_start - 1] != b'_';
    let after = key_start + key_len;
    let after_ok = after >= line.len()
        || !line.as_bytes()[after].is_ascii_alphanumeric() && line.as_bytes()[after] != b'_';
    before_ok && after_ok
}

fn find_value_end(line: &str, value_start: usize) -> usize {
    let bytes = line.as_bytes();
    if value_start >= bytes.len() {
        return value_start;
    }
    if bytes[value_start] == b'"' || bytes[value_start] == b'\'' {
        let quote = bytes[value_start];
        let mut i = value_start + 1;
        while i < bytes.len() {
            if bytes[i] == quote {
                return i + 1;
            }
            i += 1;
        }
        return bytes.len();
    }
    let mut i = value_start;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() || bytes[i] == b',' {
            break;
        }
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::redact_bundle_text;

    #[test]
    fn redacts_old_wifi_join_log_but_keeps_lengths() {
        let input = "wifi: starting join to MyNetwork (ssid_len=9 pass_len=12)\n";
        let redacted = redact_bundle_text(input);
        assert_eq!(
            redacted,
            "wifi: starting join to <redacted> (ssid_len=9 pass_len=12)\n"
        );
    }

    #[test]
    fn redacts_password_like_values_without_touching_len_fields() {
        let input = "ssid_len=9 pass_len=12 password=hunter2 token: abc123 ssid='Cafe WiFi'";
        let redacted = redact_bundle_text(input);
        assert!(redacted.contains("ssid_len=9"));
        assert!(redacted.contains("pass_len=12"));
        assert!(redacted.contains("password=<redacted>"));
        assert!(redacted.contains("token: <redacted>"));
        assert!(redacted.contains("ssid=<redacted>"));
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("Cafe WiFi"));
    }
}
