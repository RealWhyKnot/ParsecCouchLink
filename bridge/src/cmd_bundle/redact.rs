//! Bundle text redaction. Keep diagnostic structure, counters, lengths,
//! timings, and failure codes while stripping values that identify the
//! operator, local network, Wi-Fi network, or credentials.

use std::net::Ipv4Addr;

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
    "hostname",
    "computername",
    "username",
    "user_name",
    "userprofile",
];

const REDACTED: &str = "<redacted>";
const REDACTED_IP: &str = "<redacted-ip>";
const REDACTED_MAC: &str = "<redacted-mac>";
const USERPROFILE: &str = "%USERPROFILE%";

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
    let line = redact_windows_user_paths(&line);
    let line = redact_private_ipv4(&line);
    let line = redact_mac_addresses(&line);
    let line = redact_hostname_label(&line);
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
    out.push_str(REDACTED);
    out.push_str(&line[value_end..]);
    out
}

fn redact_windows_user_paths(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < line.len() {
        let Some((start, prefix)) = find_next_user_path_prefix(line, index) else {
            out.push_str(&line[index..]);
            break;
        };
        out.push_str(&line[index..start]);
        let mut user_end = start + prefix.len();
        let bytes = line.as_bytes();
        while user_end < bytes.len() && bytes[user_end] != b'\\' && bytes[user_end] != b'/' {
            user_end += 1;
        }
        out.push_str(USERPROFILE);
        index = user_end;
    }
    out
}

fn find_next_user_path_prefix(line: &str, offset: usize) -> Option<(usize, &'static str)> {
    const PREFIXES: &[&str] = &["C:\\Users\\", "C:/Users/", "C:\\\\Users\\\\"];
    let lower = line[offset..].to_ascii_lowercase();
    PREFIXES
        .iter()
        .filter_map(|prefix| {
            lower
                .find(&prefix.to_ascii_lowercase())
                .map(|pos| (offset + pos, *prefix))
        })
        .min_by_key(|(pos, _)| *pos)
}

fn redact_private_ipv4(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    let mut last = 0usize;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let end = index;
        if is_private_ipv4_token(line, start, end) {
            out.push_str(&line[last..start]);
            out.push_str(REDACTED_IP);
            last = end;
        }
    }
    if last == 0 {
        return line.to_string();
    }
    out.push_str(&line[last..]);
    out
}

fn is_private_ipv4_token(line: &str, start: usize, end: usize) -> bool {
    if start > 0 && is_token_char(line.as_bytes()[start - 1]) {
        return false;
    }
    if end < line.len() && is_token_char(line.as_bytes()[end]) {
        return false;
    }
    let candidate = &line[start..end];
    if candidate.as_bytes().iter().filter(|b| **b == b'.').count() != 3 {
        return false;
    }
    let Ok(ip) = candidate.parse::<Ipv4Addr>() else {
        return false;
    };
    is_private_or_local_ipv4(ip)
}

fn is_private_or_local_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
        || (octets[0] == 169 && octets[1] == 254)
        || octets[0] == 127
}

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
}

fn redact_mac_addresses(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    let mut last = 0usize;
    while index + 17 <= bytes.len() {
        if is_mac_at(bytes, index) {
            out.push_str(&line[last..index]);
            out.push_str(REDACTED_MAC);
            index += 17;
            last = index;
        } else {
            index += 1;
        }
    }
    if last == 0 {
        return line.to_string();
    }
    out.push_str(&line[last..]);
    out
}

fn is_mac_at(bytes: &[u8], start: usize) -> bool {
    if start > 0 && is_token_char(bytes[start - 1]) {
        return false;
    }
    let sep = bytes[start + 2];
    if sep != b':' && sep != b'-' {
        return false;
    }
    for group in 0..6 {
        let pos = start + group * 3;
        if !bytes[pos].is_ascii_hexdigit() || !bytes[pos + 1].is_ascii_hexdigit() {
            return false;
        }
        if group < 5 && bytes[pos + 2] != sep {
            return false;
        }
    }
    let end = start + 17;
    end >= bytes.len() || !is_token_char(bytes[end])
}

fn redact_hostname_label(line: &str) -> String {
    const LABELS: &[&str] = &[
        "hostname",
        "host name",
        "computername",
        "computer name",
        "username",
        "user name",
    ];
    let trimmed = line.trim_start();
    let prefix_len = line.len() - trimmed.len();
    let lower = trimmed.to_ascii_lowercase();
    for label in LABELS {
        if !lower.starts_with(label) {
            continue;
        }
        let mut value_start = prefix_len + label.len();
        let bytes = line.as_bytes();
        if value_start < bytes.len()
            && !bytes[value_start].is_ascii_whitespace()
            && bytes[value_start] != b':'
            && bytes[value_start] != b'='
        {
            continue;
        }
        while value_start < bytes.len()
            && (bytes[value_start].is_ascii_whitespace()
                || bytes[value_start] == b':'
                || bytes[value_start] == b'=')
        {
            value_start += 1;
        }
        if value_start >= bytes.len() {
            return line.to_string();
        }
        let mut out = String::with_capacity(line.len());
        out.push_str(&line[..value_start]);
        out.push_str(REDACTED);
        return out;
    }
    line.to_string()
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
            out.push_str(REDACTED);
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
            if bytes[i] == quote && quote_ends_value(bytes, i + 1) {
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

fn quote_ends_value(bytes: &[u8], next: usize) -> bool {
    next >= bytes.len()
        || bytes[next].is_ascii_whitespace()
        || matches!(bytes[next], b',' | b';' | b'}' | b']')
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

    #[test]
    fn redacts_quoted_ssid_with_apostrophe() {
        let input = "ssid='Landen's_Router_2.4G' pass_len=12";
        let redacted = redact_bundle_text(input);

        assert_eq!(redacted, "ssid=<redacted> pass_len=12");
        assert!(!redacted.contains("Landen"));
        assert!(!redacted.contains("Router"));
    }

    #[test]
    fn redacts_local_network_and_machine_identity() {
        let input = concat!(
            "ack from 192.168.50.227:4242 via 10.1.2.3\n",
            "public dns 8.8.8.8 remains visible\n",
            "path=C:\\Users\\Landen\\AppData\\Local\\ParsecCouchLink\\data\\logs\n",
            "mac=AA-BB-CC-DD-EE-FF host=02E22DA9\n",
            "hostname   DESKTOP-12345\n"
        );
        let redacted = redact_bundle_text(input);

        assert!(redacted.contains("<redacted-ip>:4242"));
        assert!(redacted.contains("via <redacted-ip>"));
        assert!(redacted.contains("public dns 8.8.8.8 remains visible"));
        assert!(
            redacted.contains("path=%USERPROFILE%\\AppData\\Local\\ParsecCouchLink\\data\\logs")
        );
        assert!(redacted.contains("mac=<redacted-mac> host=02E22DA9"));
        assert!(redacted.contains("hostname   <redacted>"));
        assert!(!redacted.contains("192.168.50.227"));
        assert!(!redacted.contains("10.1.2.3"));
        assert!(!redacted.contains("C:\\Users\\Landen"));
        assert!(!redacted.contains("AA-BB-CC-DD-EE-FF"));
        assert!(!redacted.contains("DESKTOP-12345"));
    }

    #[test]
    fn redacts_json_escaped_windows_profile_paths() {
        let input =
            r#"{ "config_path": "C:\\Users\\Landen\\AppData\\Roaming\\ParsecCouchLink\\config" }"#;
        let redacted = redact_bundle_text(input);

        assert!(redacted.contains(r#""config_path": "%USERPROFILE%\\AppData\\Roaming"#));
        assert!(!redacted.contains(r#"C:\\Users\\Landen"#));
    }
}
