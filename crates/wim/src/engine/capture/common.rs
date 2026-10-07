//! Shared capture configuration and platform-neutral scan events.
use std::path::Path;
use wim_format::ParseError;

/// Capture configuration exclusions and exceptions, in canonical UTF-8 form.
#[derive(Debug, Default)]
pub struct CaptureConfig {
    /// Patterns excluding a node and its subtree.
    pub exclusions: Vec<Vec<u8>>,
    /// Exception patterns which retain matching paths and their ancestors.
    pub exceptions: Vec<Vec<u8>>,
}
/// Borrowed scan event, translated by the C facade into its progress union.
pub struct ScanEvent<'a> {
    /// Original progress message number (9, 10, 11, or 30).
    pub message: i32,
    /// Source being scanned.
    pub source: &'a Path,
    /// Current filesystem path, absent before any node is scanned.
    pub current_path: Option<&'a Path>,
    /// Original dentry status (0 through 4).
    pub status: i32,
    /// Absolute symbolic-link target during fixup notifications.
    pub symlink_target: Option<&'a [u8]>,
    /// Progress-visible directory count.
    pub directories: u64,
    /// Progress-visible non-directory count.
    pub nondirectories: u64,
    /// Progress-visible unique stream bytes.
    pub bytes: u64,
    /// Bidirectional file exclusion decision for message 30.
    pub exclude: bool,
    /// Error code on message 31; callback may set exclude to ignore it.
    pub error: Option<ParseError>,
}
pub(super) type ScanCallback<'a> = dyn FnMut(&mut ScanEvent<'_>) -> Result<(), ParseError> + 'a;

impl CaptureConfig {
    /// Parse already translated capture configuration text. Compression and
    /// prepopulation sections are recognized but have no capture effect.
    pub fn parse_text(text: &[u8]) -> Result<Self, ParseError> {
        let mut config = Self::default();
        let mut section = 0;
        for line in text.split(|&b| b == b'\n') {
            let mut line = line;
            while line.first().is_some_and(u8::is_ascii_whitespace) {
                line = &line[1..];
            }
            while line.last().is_some_and(u8::is_ascii_whitespace) {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() || matches!(line[0], b'#' | b';') {
                continue;
            }
            if line[0] == b'[' {
                let Some(end) = line.iter().position(|&b| b == b']') else {
                    return Err(ParseError::InvalidCaptureConfig);
                };
                section = if line[1..end].eq_ignore_ascii_case(b"ExclusionList") {
                    1
                } else if line[1..end].eq_ignore_ascii_case(b"ExclusionException") {
                    2
                } else {
                    0
                };
                continue;
            }
            if section == 0 {
                continue;
            }
            if line.len() >= 2 && matches!(line[0], b'\'' | b'"') && line.last() == line.first() {
                line = &line[1..line.len() - 1];
            }
            if let Some(end) = line.iter().position(|&b| b == 0) {
                line = &line[..end];
            }
            let mut pattern = Vec::new();
            if line.get(1) == Some(&b':') {
                if !matches!(line.get(2), Some(b'/' | b'\\')) {
                    return Err(ParseError::InvalidCaptureConfig);
                }
                line = &line[2..];
            }
            for &byte in line {
                let byte = if byte == b'\\' { b'/' } else { byte };
                if byte != b'/' || pattern.last() != Some(&b'/') {
                    pattern.push(byte);
                }
            }
            if pattern.first() != Some(&b'/') && pattern.contains(&b'/') {
                return Err(ParseError::InvalidCaptureConfig);
            }
            // Pattern conversion failures are configuration errors, not raw text errors.
            wim_format::platform_text::wtf8_to_utf16(&pattern)
                .map_err(|_| ParseError::InvalidCaptureConfig)?;
            if section == 1 {
                config.exclusions.push(pattern);
            } else {
                config.exceptions.push(pattern);
            }
        }
        Ok(config)
    }
    /// Original built-in Windows exclusions selected by WINCONFIG.
    pub fn windows_default() -> Self {
        Self {
            exclusions: [
                "/$ntfs.log",
                "/hiberfil.sys",
                "/pagefile.sys",
                "/swapfile.sys",
                "/System Volume Information",
                "/RECYCLER",
                "/$RECYCLE.BIN",
                "/$Recycle.Bin",
                "/Windows/CSC",
            ]
            .into_iter()
            .map(|p| p.as_bytes().to_vec())
            .collect(),
            exceptions: Vec::new(),
        }
    }
    pub(super) fn excluded(&self, relative: &[u8]) -> bool {
        let mut path = vec![b'/'];
        path.extend_from_slice(relative.strip_prefix(b"/").unwrap_or(relative));
        self.exclusions
            .iter()
            .any(|p| pattern_matches(p, &path, false))
            && !self
                .exceptions
                .iter()
                .any(|p| pattern_matches(p, &path, true))
    }
}
fn wildcard(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len()
            && (pattern[p] == b'?'
                || (pattern[p] == value[v]
                    || (crate::engine::runtime::ignore_case()
                        && pattern[p].eq_ignore_ascii_case(&value[v]))))
        {
            p += 1;
            v += 1;
        } else if pattern.get(p) == Some(&b'*') {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(s) = star {
            retry += 1;
            v = retry;
            p = s + 1;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}
fn pattern_matches(pattern: &[u8], path: &[u8], ancestors: bool) -> bool {
    let parts: Vec<_> = path
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .collect();
    let path = if pattern.starts_with(b"/") {
        &parts[..]
    } else {
        &parts[parts.len().saturating_sub(1)..]
    };
    let pattern: Vec<_> = pattern
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .collect();
    pattern.iter().zip(path).all(|(p, v)| wildcard(p, v))
        && (pattern.len() <= path.len() || ancestors)
}
