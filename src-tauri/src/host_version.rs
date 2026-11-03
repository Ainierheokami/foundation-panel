//! What the panel keeps of the host's version: only text that can be a version, read from `sunshine --version [--json]`.
//!
//! The version is shown in the caption bar and copied into bug reports, so a string that is not a version (console text read with the wrong
//! code page, a log line, an error message) must never get there: it is dropped and the version reads "Unknown".

use std::time::SystemTime;

/// A version is at most this long (`2026.1112.025657.970e0298b.杂鱼` is 31).
const MAX_VERSION_CHARS: usize = 64;

/// Whether `text` can be a Foundation Sunshine version: `[v]<digit>...` or `manual-...`, made of ASCII letters and digits, `. - _ +`, and CJK
/// ideographs, kana or hangul (the `.杂鱼` suffix). Latin-1 and Latin Extended letters (GBK bytes read as Latin-1: "ÏµÍ³"), Greek and Cyrillic
/// (GBK bytes that happen to be valid UTF-8: "ϵͳ"), the replacement character, control characters, spaces and everything else are not.
pub(crate) fn is_plain_version_text(text: &str) -> bool {
    if text.is_empty() || text.chars().count() > MAX_VERSION_CHARS {
        return false;
    }
    let body = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let starts_like_a_version = body.starts_with(|c: char| c.is_ascii_digit())
        || body
            .get(..7)
            .is_some_and(|head| head.eq_ignore_ascii_case("manual-"));
    starts_like_a_version
        && text.chars().all(|c| {
            if c.is_ascii() {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+')
            } else {
                matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF)
            }
        })
}

/// The version a host printed for `--version [--json]`: the `version` of the JSON line, else the text after `version:` of the plain line of an
/// older host, whole (never cut at the first letter: ".970e0298b" and "manual-..." stay what the host wrote). `None` when there is neither,
/// or when what is there is not plain version text.
pub(crate) fn parse_version_output(output: &str) -> Option<String> {
    let version = if let Some(line) = output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('{'))
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(line)
    {
        json.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        output.lines().find_map(|line| {
            let (_, rest) = line.split_once("version:")?;
            let token = rest.split_whitespace().next()?;
            Some(token.strip_prefix(['v', 'V']).unwrap_or(token).to_string())
        })?
    };
    is_plain_version_text(&version).then_some(version)
}

/// Identity of an executable file on disk: when it changed, the program was replaced (an update or a reinstall) and what was read from the old
/// one is stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    pub modified: SystemTime,
    pub len: u64,
}

impl FileStamp {
    pub(crate) fn of(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }
}

/// Remember `now` in `seen`; true when a different stamp was remembered before (the first sighting is not a change).
pub(crate) fn replaced_since(seen: &mut Option<FileStamp>, now: Option<FileStamp>) -> bool {
    let changed = matches!((*seen, now), (Some(before), Some(after)) if before != after);
    if now.is_some() {
        *seen = now;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn json_version_is_read_whole() {
        // `--version --json`: whatever follows the date stays, nothing is cut at the first letter
        for version in [
            "2026.1112.025657.杂鱼",
            "2026.1112.025657.970e0298b",
            "2026.1112.025657.abcdef1",
            "2026.1112.025657.970e0298b.杂鱼",
            "manual-20261112-025657-dev",
        ] {
            let line = format!(r#"{{"version":"{version}","commit":"","dirty":false}}"#);
            assert_eq!(
                parse_version_output(&line).as_deref(),
                Some(version),
                "{version}"
            );
        }
    }

    #[test]
    fn json_line_wins_over_other_output_and_needs_a_version() {
        let output = "some banner\n{\"version\":\" 2026.1112.025657 \"}\n";
        assert_eq!(
            parse_version_output(output).as_deref(),
            Some("2026.1112.025657")
        );
        assert_eq!(parse_version_output("{\"commit\":\"abc\"}"), None);
        assert_eq!(parse_version_output("{\"version\":\"\"}"), None);
    }

    #[test]
    fn an_older_host_prints_a_plain_line() {
        assert_eq!(
            parse_version_output("Sunshine version: 2026.1104.121813.杂鱼\n").as_deref(),
            Some("2026.1104.121813.杂鱼")
        );
        assert_eq!(
            parse_version_output("Sunshine version: v2026.1103.21122.杂鱼").as_deref(),
            Some("2026.1103.21122.杂鱼")
        );
        assert_eq!(
            parse_version_output("Sunshine version: 2026.1112.025657.970e0298b").as_deref(),
            Some("2026.1112.025657.970e0298b")
        );
        assert_eq!(parse_version_output("no version here"), None);
        assert_eq!(parse_version_output(""), None);
    }

    /// The copied bug-report block must never carry console text: GBK "系统" (CF B5 CD B3) read as Latin-1 is "ÏµÍ³", read lossily is U+FFFD.
    #[test]
    fn console_text_is_never_a_version() {
        let gbk: [u8; 4] = [0xCF, 0xB5, 0xCD, 0xB3];
        let as_latin1: String = gbk.iter().map(|&b| b as char).collect();
        assert_eq!(as_latin1, "ÏµÍ³");
        let lossy = String::from_utf8_lossy(&gbk).into_owned();
        for garbage in [
            as_latin1.clone(),
            lossy.clone(),
            format!("2026.1112.165752.{as_latin1}"),
            format!("2026.1112.165752 {as_latin1}"),
            format!("2026.1112.165752.{lossy}"),
            "系统找不到指定的文件。".to_string(),
            "2026.1112.165752\n系统: Windows".to_string(),
            "2026.1112.165752\0".to_string(),
        ] {
            assert!(!is_plain_version_text(&garbage), "{garbage:?}");
            let json = serde_json::json!({ "version": garbage }).to_string();
            assert_eq!(parse_version_output(&json), None, "{garbage:?}");
        }
        // a plain line of an older host whose version token is console text
        assert_eq!(
            parse_version_output(&format!("Sunshine version: {as_latin1}")),
            None
        );
        assert_eq!(
            parse_version_output(&format!("Sunshine version: {lossy}")),
            None
        );
        // a log line on the other stream never supplies a version
        assert_eq!(
            parse_version_output("[2026-11-12 17:05:15] Error: version: 系统 failed"),
            None
        );
    }

    /// The same on Windows with the real decoder: what a Chinese console writes (GBK) is decoded with the OEM code page, and is still no version.
    #[cfg(target_os = "windows")]
    #[test]
    fn gbk_console_output_is_decoded_and_is_never_a_version() {
        let gbk = [
            0xcf, 0xb5, 0xcd, 0xb3, 0xd5, 0xd2, 0xb2, 0xbb, 0xb5, 0xbd, 0xd6, 0xb8, 0xb6, 0xa8,
            0xb5, 0xc4, 0xce, 0xc4, 0xbc, 0xfe, 0xa1, 0xa3,
        ];
        let text = crate::text_encoding::decode_console_bytes(&gbk);
        assert_eq!(text, "系统找不到指定的文件。");
        assert_eq!(parse_version_output(&text), None);
        assert_eq!(parse_version_output(&format!("version: {text}")), None);
    }

    #[test]
    fn real_versions_pass_the_plain_text_check() {
        for version in [
            "2026.1112.165752",
            "v2026.1103.21122.杂鱼",
            "2026.1112.025657.970e0298b.杂鱼",
            "manual-20261112-025657-dev",
            "2026.1112.025657.のうち",
            "1.0.0",
        ] {
            assert!(is_plain_version_text(version), "{version}");
        }
        for not_a_version in ["", "Unknown", "未知", "v", "-1.0", ".1.2.3"] {
            assert!(!is_plain_version_text(not_a_version), "{not_a_version:?}");
        }
        assert!(!is_plain_version_text(&"1".repeat(65)));
    }

    #[test]
    fn a_replaced_host_is_noticed_once() {
        let at = |secs: u64, len: u64| {
            Some(FileStamp {
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
                len,
            })
        };
        let mut seen = None;
        // the first sighting is not a change; the same file again is not either
        assert!(!replaced_since(&mut seen, at(100, 5)));
        assert!(!replaced_since(&mut seen, at(100, 5)));
        // a newer file (a reinstall) or another size is
        assert!(replaced_since(&mut seen, at(200, 5)));
        assert!(!replaced_since(&mut seen, at(200, 5)));
        assert!(replaced_since(&mut seen, at(200, 6)));
        // a file that cannot be read for a moment (being replaced) does not forget the last one
        assert!(!replaced_since(&mut seen, None));
        assert!(!replaced_since(&mut seen, at(200, 6)));
    }
}
