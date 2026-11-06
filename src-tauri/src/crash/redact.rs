//! Privacy pass over the bytes of a minidump before it leaves the computer (the same rules as the host's `crash_core.cpp` `redact_private_names`).
//!
//! The dump type already keeps heap and string data out (`win.rs`: stack pointers only, module paths filtered). What can be left is a path
//! (the module list, a stack word that points at a string). Blanked in place with `x`, same length so the file stays valid, in ASCII/UTF-8
//! and in UTF-16LE: the account name behind a `\Users\`, `/Users/` or `/home/` marker (up to the next separator), and every whole-token
//! occurrence (not inside a longer word) of the names given (account, computer), ignoring ASCII case. Names under 3 bytes are ignored.

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn fold(byte: u8) -> u8 {
    byte.to_ascii_lowercase()
}

fn matches_at(data: &[u8], at: usize, pattern: &[u8]) -> bool {
    data.len() >= at + pattern.len() && data[at..at + pattern.len()].iter().zip(pattern).all(|(a, b)| fold(*a) == fold(*b))
}

fn path_stop(unit: u32) -> bool {
    unit < 0x20 || unit == 0x7f || matches!(char::from_u32(unit), Some('\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

fn word_unit(data: &[u8], at: usize, unit: usize) -> bool {
    if at + unit > data.len() || (unit == 2 && data[at + 1] != 0) {
        return false;
    }
    let c = data[at];
    c.is_ascii_alphanumeric() || c == b'_'
}

fn blank(data: &mut [u8], from: usize, units: usize, unit: usize) {
    for k in 0..units {
        data[from + k * unit] = b'x';
        if unit == 2 {
            data[from + k * unit + 1] = 0;
        }
    }
}

fn blank_after_marker(data: &mut [u8], marker: &[u8], unit: usize) -> usize {
    let mut hits = 0;
    let mut i = 0;
    while i + marker.len() < data.len() {
        if !matches_at(data, i, marker) {
            i += 1;
            continue;
        }
        let start = i + marker.len();
        let mut units = 0;
        let mut all_x = true;
        let mut j = start;
        while units < 64 && j + unit <= data.len() {
            let u = if unit == 2 { u32::from(data[j]) | (u32::from(data[j + 1]) << 8) } else { u32::from(data[j]) };
            if path_stop(u) {
                break;
            }
            if u != u32::from(b'x') {
                all_x = false;
            }
            units += 1;
            j += unit;
        }
        if units > 0 && !all_x {
            blank(data, start, units, unit);
            hits += 1;
        }
        i = start + units * unit;
    }
    hits
}

fn blank_token(data: &mut [u8], pattern: &[u8], unit: usize) -> usize {
    let mut hits = 0;
    let first = fold(pattern[0]);
    let mut i = 0;
    while i + pattern.len() <= data.len() {
        if fold(data[i]) != first || !matches_at(data, i, pattern) || (i >= unit && word_unit(data, i - unit, unit)) || word_unit(data, i + pattern.len(), unit) {
            i += 1;
            continue;
        }
        blank(data, i, pattern.len() / unit, unit);
        hits += 1;
        i += pattern.len();
    }
    hits
}

/// Blanks account and computer names in `data`; returns how many places were blanked.
pub(crate) fn redact_private_names(data: &mut [u8], names: &[String]) -> usize {
    let mut hits = 0;
    for marker in ["\\Users\\", "/Users/", "/home/"] {
        hits += blank_after_marker(data, marker.as_bytes(), 1);
        hits += blank_after_marker(data, &utf16le(marker), 2);
    }
    for name in names {
        if name.len() < 3 {
            continue;
        }
        hits += blank_token(data, name.as_bytes(), 1);
        hits += blank_token(data, &utf16le(name), 2);
    }
    hits
}

/// What names this account and this computer (environment variables; nothing is read from the person's files).
pub(crate) fn private_names() -> Vec<String> {
    ["USERNAME", "USER", "LOGNAME", "COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("system") && !value.eq_ignore_ascii_case("root"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u8> {
        utf16le(text)
    }

    fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
        data.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn account_name_behind_a_home_marker_is_blanked_in_ascii_and_utf16() {
        let mut data = b"MDMP\0\0C:\\Users\\alice\\AppData\\sunshine.exe\0\0\0/home/bob/.config\0\0/Users/carol Smith/x".to_vec();
        data.extend(wide("C:\\Users\\dave\\Desktop\\sunshine.exe"));
        let before = data.len();
        assert_eq!(redact_private_names(&mut data, &[]), 4);
        assert_eq!(data.len(), before);
        for secret in ["alice", "bob", "carol", "Smith"] {
            assert!(find(&data, secret.as_bytes()).is_none(), "{secret}");
        }
        assert!(find(&data, &wide("dave")).is_none());
        assert!(find(&data, b"\\Users\\xxxxx\\AppData\\sunshine.exe").is_some());
        assert!(find(&data, &wide("\\Users\\xxxx\\Desktop\\sunshine.exe")).is_some());
    }

    #[test]
    fn a_non_ascii_account_name_in_utf16() {
        let mut data = wide("C:\\Users\\");
        data.extend([0x0f, 0x5c, 0x0e, 0x66]);
        data.extend(wide("\\Documents"));
        redact_private_names(&mut data, &[]);
        assert!(find(&data, &[0x0f, 0x5c]).is_none());
        assert!(find(&data, &wide("\\Documents")).is_some());
    }

    #[test]
    fn known_names_are_blanked_as_whole_tokens_only() {
        let mut data = b"host SKYPC-01 and skypc-01; user Sky Hua; sunshine.exe SkyHua2".to_vec();
        data.extend(wide("SKYPC-01"));
        data.extend(wide(" "));
        data.extend(wide("skyhua"));
        let names = ["SkyPC-01", "SkyHua", "sun", "ab"].map(String::from);
        assert_eq!(redact_private_names(&mut data, &names), 4);
        assert!(find(&data, b"SKYPC-01").is_none());
        assert!(find(&data, b"skypc-01").is_none());
        assert!(find(&data, &wide("SKYPC-01")).is_none());
        assert!(find(&data, &wide("skyhua")).is_none());
        assert!(find(&data, b"sunshine.exe").is_some());
        assert!(find(&data, b"SkyHua2").is_some());
    }

    #[test]
    fn a_dump_with_nothing_private_is_left_as_it_was() {
        let mut data = [b"MDMP".as_slice(), &[0x41; 200], b"sunshine.exe", &[0; 50]].concat();
        let copy = data.clone();
        assert_eq!(redact_private_names(&mut data, &["alice".to_string()]), 0);
        assert_eq!(data, copy);
    }
}
