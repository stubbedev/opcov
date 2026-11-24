use crate::report::Sha256;

const EVAL_DIGEST_BYTES: usize = 8;

#[must_use]
pub fn eval_host(reported: &str) -> Option<&str> {
    let mut host = compiling_file(reported)?;
    while let Some(outer) = compiling_file(host) {
        host = outer;
    }
    return Some(host);
}

fn compiling_file(description: &str) -> Option<&str> {
    let (located, name) = description.rsplit_once(") : ")?;
    let (file, line) = located.rsplit_once('(')?;
    if name.is_empty() || file.is_empty() || line.is_empty() {
        return None;
    }
    if !line.bytes().all(|byte| return byte.is_ascii_digit()) {
        return None;
    }
    return Some(file);
}

#[must_use]
pub fn stream_scheme(path: &str) -> Option<&str> {
    let length = path
        .bytes()
        .take_while(|&byte| {
            return byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.');
        })
        .count();
    if length < 2 {
        return None;
    }
    let (scheme, rest) = path.split_at_checked(length)?;
    if rest.starts_with("://") || (scheme == "data" && rest.starts_with(':')) {
        return Some(scheme);
    }
    return None;
}

#[must_use]
pub fn eval_source(compiled: &str, sha256: &Sha256) -> String {
    let digits = sha256
        .as_bytes()
        .iter()
        .take(EVAL_DIGEST_BYTES)
        .flat_map(|&byte| return [byte >> 4_u8, byte & 0x0f])
        .filter_map(|nibble| return char::from_digit(u32::from(nibble), 16));
    let mut name = String::with_capacity(
        compiled
            .len()
            .saturating_add(2)
            .saturating_add(EVAL_DIGEST_BYTES.saturating_mul(2)),
    );
    name.push_str(compiled);
    name.push_str(" #");
    name.extend(digits);
    return name;
}
