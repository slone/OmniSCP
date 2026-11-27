// Directory listings in the `ls -l` format, which SCP gets from `ls` and most
// FTP servers send for `LIST`.

use std::time::UNIX_EPOCH;

use crate::protocols::parse_permissions;

pub(crate) fn now() -> i64 {

    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LsLine {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub mode: u32,
    /// With setuid, setgid and sticky bits, for display and chmod.
    pub full_mode: u32,
    pub owner: String,
    pub group: String,
    pub size: u64,
    pub modified: Option<i64>,
    pub name: String,
    /// Where a symlink points, as `ls` shows it after ` -> `.
    pub link_target: Option<String>,
}

/// Permission bits of a string like `drwxr-sr-x`; special bits are not kept.
pub(crate) fn mode_bits(permissions: &str) -> u32 {
    permissions
        .chars()
        .skip(1)
        .take(9)
        .fold(0, |mode, c| mode << 1 | u32::from(matches!(c, 'r' | 'w' | 'x' | 's' | 't')))
}

/// Splits off the next whitespace-separated word, keeping the rest verbatim for names.
fn next_word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start_matches(' ');
    let end = text.find(' ').unwrap_or(text.len());
    if end == 0 { None } else { Some((&text[..end], &text[end..])) }
}

pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Howard Hinnant's algorithm, for the proleptic Gregorian calendar.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_number(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    MONTHS.iter().position(|m| *m == name).map(|i| i as i64 + 1)
}

fn clock_seconds(clock: &str) -> Option<i64> {
    let mut parts = clock.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: i64 = parts.next().map_or(Some(0), |s| s.split('.').next()?.parse().ok())?;
    Some(hours * 3600 + minutes * 60 + seconds)
}

/// Reads the date of an `ls` line; returns it with what follows. Which form appears
/// depends on the `ls` variant. Only the epoch form is exact across time zones.
fn parse_ls_date(text: &str, now: i64) -> Option<(i64, &str)> {
    let (first, rest) = next_word(text)?;
    // 1790000000
    if first.bytes().all(|b| b.is_ascii_digit()) {
        return Some((first.parse().ok()?, rest));
    }
    // 2026-11-05 16:17:03 +0000
    let mut ymd = first.split('-');
    if let (Some(y), Some(m), Some(d), None) = (ymd.next(), ymd.next(), ymd.next(), ymd.next()) {
        let days = days_from_civil(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        let (clock, rest) = next_word(rest)?;
        let (zone, rest) = next_word(rest)?;
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let digits: i64 = zone.trim_start_matches(['+', '-']).parse().ok()?;
        let offset = sign * (digits / 100 * 3600 + digits % 100 * 60);
        return Some((days * 86400 + clock_seconds(clock)? - offset, rest));
    }
    // Thu Nov  5 16:17:03 2026
    if month_number(first).is_none() {
        let (month, rest) = next_word(rest)?;
        let (day, rest) = next_word(rest)?;
        let (clock, rest) = next_word(rest)?;
        let (year, rest) = next_word(rest)?;
        let days = days_from_civil(year.parse().ok()?, month_number(month)?, day.parse().ok()?);
        return Some((days * 86400 + clock_seconds(clock)?, rest));
    }
    // Nov  5 16:17 (within about six months), or Nov  5  2024
    let month = month_number(first)?;
    let (day, rest) = next_word(rest)?;
    let day: i64 = day.parse().ok()?;
    let (third, rest) = next_word(rest)?;
    if third.contains(':') {
        let clock = clock_seconds(third)?;
        let at = |year| days_from_civil(year, month, day) * 86400 + clock;
        // No year means recent: the latest year that does not put the date in the future.
        let mut year = 1971 + now.div_euclid(31_556_952);
        while at(year) > now + 86400 {
            year -= 1;
        }
        Some((at(year), rest))
    } else {
        Some((days_from_civil(third.parse().ok()?, month, day) * 86400, rest))
    }
}

/// Undoes GNU `ls -b`: `\ `, `\\`, `\n`, octal bytes such as `\303\251`.
fn unescape_ls_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 == bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        let octal = bytes.get(i + 1..i + 4).filter(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)));
        if let Some(digits) = octal {
            out.push(digits.iter().fold(0u32, |v, b| v * 8 + u32::from(b - b'0')) as u8);
            i += 4;
            continue;
        }
        out.push(match next {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'v' => 0x0b,
            other => other,
        });
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses a line of `ls -ln`, such as:
/// `drwxr-xr-x 2 1000 1000 4096 1790000000 name`.
pub(crate) fn parse_ls_line(line: &str, escaped: bool, now: i64) -> Option<LsLine> {
    let (permissions, rest) = next_word(line)?;
    let kind = permissions.chars().next()?;
    if permissions.len() < 10 || !"-dlbcps".contains(kind) {
        return None;
    }
    let (_links, rest) = next_word(rest)?;
    let (owner, rest) = next_word(rest)?;
    let (group, rest) = next_word(rest)?;
    let (size, rest) = next_word(rest)?;
    // Devices show "major, minor" instead of a size.
    let (size, rest) = if size.ends_with(',') {
        (0, next_word(rest)?.1)
    } else if size.contains(',') {
        (0, rest)
    } else {
        (size.parse().ok()?, rest)
    };
    let (modified, rest) = parse_ls_date(rest, now)?;

    // One separator: anything after it belongs to the name.
    let name = rest.strip_prefix(' ')?;
    let (name, link_target) = match name.split_once(" -> ") {
        Some((name, target)) if kind == 'l' => (name, Some(target)),
        _ => (name, None),
    };
    if name.is_empty() {
        return None;
    }
    Some(LsLine {
        is_dir: kind == 'd',
        is_symlink: kind == 'l',
        mode: mode_bits(permissions),
        full_mode: parse_permissions(permissions),
        owner: owner.to_string(),
        group: group.to_string(),
        size,
        modified: Some(modified),
        name: if escaped { unescape_ls_name(name) } else { name.to_string() },
        link_target: link_target.map(|t| if escaped { unescape_ls_name(t) } else { t.to_string() }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-05 12:00:00 UTC.
    const NOW: i64 = 1_793_880_000;

    fn parse(line: &str) -> LsLine {
        parse_ls_line(line, false, NOW).unwrap_or_else(|| panic!("could not parse {line:?}"))
    }

    #[test]
    fn gnu_line_with_epoch() {
        let line = parse("-rw-r--r-- 1 1000 1001 12345 1790000000 report.pdf");
        assert_eq!(line.name, "report.pdf");
        assert_eq!(line.size, 12345);
        assert_eq!(line.modified, Some(1790000000));
        assert_eq!((line.owner.as_str(), line.group.as_str()), ("1000", "1001"));
        assert_eq!(line.mode, 0o644);
        assert!(!line.is_dir && !line.is_symlink);
    }

    #[test]
    fn folders_links_and_special_files() {
        let dir = parse("drwxr-xr-x 2 0 0 4096 1790000000 sub");
        assert!(dir.is_dir);
        assert_eq!(dir.mode, 0o755);

        let link = parse("lrwxrwxrwx 1 0 0 4 1790000000 to dir -> target with -> arrow");
        assert!(link.is_symlink);
        assert_eq!(link.name, "to dir");
        assert_eq!(link.link_target.as_deref(), Some("target with -> arrow"));

        let device = parse("crw-rw-rw- 1 0 0 1, 3 1790000000 null");
        assert_eq!((device.size, device.name.as_str()), (0, "null"));
        let packed = parse("crw-rw-rw- 1 0 0 1,3 1790000000 zero");
        assert_eq!((packed.size, packed.name.as_str()), (0, "zero"));

        // ACL and SELinux markers after the permissions.
        assert_eq!(parse("-rw-r--r--+ 1 0 0 1 1790000000 a").name, "a");
        assert_eq!(parse("-rw-r--r--. 1 0 0 1 1790000000 b").name, "b");
    }

    #[test]
    fn names_keep_their_spaces() {
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 1790000000 two  spaces ").name, "two  spaces ");
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 1790000000  leading").name, " leading");
    }

    #[test]
    fn every_date_form() {
        // Exact to the second, and agreeing with each other.
        let epoch = parse("-rw-r--r-- 1 0 0 1 1793896623 a").modified;
        assert_eq!(epoch, Some(1793896623));
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 16:37:03 +0000 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 18:37:03.123456 +0200 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 11:07:03 -0530 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Thu Nov  5 16:37:03 2026 a").modified, epoch);

        // Plain ls: to the minute.
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Nov  5 11:37 a").modified, Some(1793878620));
        // A date ahead of now without a year is from last year.
        let december = parse("-rw-r--r-- 1 0 0 1 Dec 25 10:00 a").modified.unwrap();
        assert!(december < NOW && NOW - december < 366 * 86400);
        // Just after New Year, December is still last year's, and January this year's.
        let new_year = days_from_civil(2027, 1, 1) * 86400 + 3600;
        let parsed = parse_ls_date("Dec 31 23:00 a", new_year).unwrap().0;
        assert_eq!(parsed, days_from_civil(2026, 12, 31) * 86400 + 23 * 3600);
        let parsed = parse_ls_date("Jan  1 00:30 a", new_year).unwrap().0;
        assert_eq!(parsed, days_from_civil(2027, 1, 1) * 86400 + 1800);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Jan  2  2024 a").modified, Some(1704153600));
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Jan  2  2024 two words").name, "two words");
    }

    #[test]
    fn gnu_escapes_are_undone() {
        let name = |line: &str| parse_ls_line(line, true, NOW).unwrap().name;
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 a\ b"), "a b");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 back\\slash"), "back\\slash");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 new\nline"), "new\nline");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 caf\303\251"), "café");
        assert_eq!(name(r"lrwxrwxrwx 1 0 0 1 1790000000 a\ ->\ b -> c"), "a -> b");
    }

    #[test]
    fn unknown_lines_are_refused_not_skipped() {
        for line in [
            "",
            "garbage",
            "total 12",
            "?????????? ? ? ? ? ? name",
            "-rw-r--r-- 1 0 0 notasize 1790000000 a",
            "-rw-r--r-- 1 0 0 1 someday a",
            "-rw-r--r-- 1 0 0 1 1790000000",
            "second half of a name with a newline",
        ] {
            assert!(parse_ls_line(line, false, NOW).is_none(), "{line:?} should be refused");
        }
    }

    #[test]
    fn permission_bits() {
        assert_eq!(mode_bits("-rwxr-x---"), 0o750);
        assert_eq!(mode_bits("drwxrwxrwt"), 0o777);
        assert_eq!(mode_bits("-rw-r--r--+"), 0o644);
        assert_eq!(mode_bits("-rw-r-Sr--"), 0o644);
    }
}
