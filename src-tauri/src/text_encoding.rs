//! Text from a child process or a Windows message table is not guaranteed to
//! be UTF-8. A console program on a Chinese Windows writes GBK; keep valid
//! UTF-8 as it is and convert the rest from the Windows OEM code page before
//! it reaches a panel log line, the log file or an export.
//!
//! Sources: panel-ms-multibytetowidechar (CP_ACP, CP_OEMCP and
//! MB_ERR_INVALID_CHARS) and panel-ms-code-page-identifiers (65001
//! and 936); the viewer's GB18030 fallback is panel-whatwg-encoding.

#[cfg(target_os = "windows")]
use windows::Win32::Globalization::{
    GetACP, GetOEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar,
};

/// Two or four bytes of GBK are often valid UTF-8 too: "系统" (CF B5 CD B3) is the Greek "ϵͳ". On a system whose console code page is CJK,
/// text that is valid UTF-8 but holds no character from U+0800 up (no CJK, kana, hangul, symbol or emoji, which a real UTF-8 program writes
/// as three or four bytes) is much more likely the console's own code page than UTF-8.
#[cfg(any(target_os = "windows", test))]
fn looks_like_oem_bytes_read_as_utf8(text: &str) -> bool {
    let mut non_ascii = text.chars().filter(|c| !c.is_ascii()).peekable();
    non_ascii.peek().is_some() && non_ascii.all(|c| (c as u32) < 0x800)
}

/// The code pages whose console programs write double-byte text (GBK, Big5, Shift-JIS, Korean).
#[cfg(any(target_os = "windows", test))]
fn is_cjk_code_page(code_page: u32) -> bool {
    matches!(code_page, 936 | 950 | 932 | 949)
}

/// Decode one captured output stream using the same rule as the viewer.
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        #[cfg(target_os = "windows")]
        {
            let oem = unsafe { GetOEMCP() };
            if is_cjk_code_page(oem) && looks_like_oem_bytes_read_as_utf8(text) {
                return decode_with_code_page(bytes, oem);
            }
        }
        return text.to_owned();
    }
    #[cfg(target_os = "windows")]
    {
        return decode_with_code_page(bytes, unsafe { GetOEMCP() });
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Decode a Windows API buffer that uses the ANSI code page (for example the
/// registered owner string of a shared-memory block).
pub fn decode_ansi_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    #[cfg(target_os = "windows")]
    {
        return decode_with_code_page(bytes, unsafe { GetACP() });
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Decode stdout and stderr of one finished child process.
pub fn decode_output(output: &std::process::Output) -> (String, String) {
    (
        decode_console_bytes(&output.stdout),
        decode_console_bytes(&output.stderr),
    )
}

#[cfg(target_os = "windows")]
fn decode_with_code_page(bytes: &[u8], code_page: u32) -> String {
    let size =
        unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
    if size <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; size as usize];
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
            bytes,
            Some(&mut wide),
        )
    };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    wide.truncate(written as usize);
    String::from_utf16_lossy(&wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_valid_utf8() {
        assert_eq!(
            decode_console_bytes("系统找不到指定的文件。".as_bytes()),
            "系统找不到指定的文件。"
        );
    }

    #[test]
    fn short_gbk_that_is_also_valid_utf8_is_recognised() {
        // "系统" in GBK is CF B5 CD B3, which is valid UTF-8 for the Greek "ϵͳ"
        let greek = std::str::from_utf8(&[0xcf, 0xb5, 0xcd, 0xb3]).unwrap();
        assert_eq!(greek, "ϵͳ");
        assert!(looks_like_oem_bytes_read_as_utf8(greek));
        assert!(looks_like_oem_bytes_read_as_utf8("é"));
        // real UTF-8 from a Chinese program has characters from U+0800 up; plain ASCII is never in doubt
        assert!(!looks_like_oem_bytes_read_as_utf8("系统找不到指定的文件。"));
        assert!(!looks_like_oem_bytes_read_as_utf8("系统 ϵ"));
        assert!(!looks_like_oem_bytes_read_as_utf8("2026.1112.165752"));
        assert!(!looks_like_oem_bytes_read_as_utf8(""));
        for page in [936, 950, 932, 949] {
            assert!(is_cjk_code_page(page));
        }
        for page in [437, 850, 1252, 65001] {
            assert!(!is_cjk_code_page(page));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn short_gbk_that_is_also_valid_utf8_is_decoded_as_gbk_on_a_cjk_system() {
        if unsafe { GetOEMCP() } == 936 {
            assert_eq!(decode_console_bytes(&[0xcf, 0xb5, 0xcd, 0xb3]), "系统");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn decodes_console_gbk() {
        let gbk = [
            0xcf, 0xb5, 0xcd, 0xb3, 0xd5, 0xd2, 0xb2, 0xbb, 0xb5, 0xbd, 0xd6, 0xb8, 0xb6, 0xa8,
            0xb5, 0xc4, 0xce, 0xc4, 0xbc, 0xfe, 0xa1, 0xa3,
        ];
        assert_eq!(decode_console_bytes(&gbk), "系统找不到指定的文件。");
    }
}
