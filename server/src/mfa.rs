//! Two-factor sign-in with an authenticator app (TOTP, RFC 6238), and
//! one-time recovery codes.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
pub const STEP_SECS: i64 = 30;
pub const DIGITS: u32 = 6;

/// RFC 4648 base32, without padding (how authenticator apps expect secrets).
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(ALPHABET[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let v = ALPHABET.iter().position(|a| *a as char == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

/// A new 160-bit secret, base32-encoded.
pub fn new_secret() -> String {
    let mut bytes = Vec::with_capacity(20);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(&uuid::Uuid::new_v4().as_bytes()[..4]);
    base32_encode(&bytes)
}

/// The code for one time step (RFC 4226 HOTP with SHA-1).
pub fn code_at(secret: &[u8], step: i64, digits: u32) -> String {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).expect("HMAC takes keys of any length");
    mac.update(&(step as u64).to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[hash.len() - 1] & 0x0f) as usize;
    let value = u32::from_be_bytes([hash[offset] & 0x7f, hash[offset + 1], hash[offset + 2], hash[offset + 3]]);
    format!("{:0width$}", value % 10u32.pow(digits), width = digits as usize)
}

/// Checks a code against the current step and one step either side (clock
/// drift). Returns the step it matched, which must be later than any code
/// already used.
pub fn verify(secret_b32: &str, code: &str, now: i64, last_step: i64) -> Option<i64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let secret = base32_decode(secret_b32)?;
    let step = now / STEP_SECS;
    [step - 1, step, step + 1]
        .into_iter()
        .filter(|s| *s > last_step)
        .find(|s| code_at(&secret, *s, DIGITS) == code)
}

/// The link authenticator apps read from the QR code.
pub fn otpauth_url(secret_b32: &str, email: &str) -> String {
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    };
    format!(
        "otpauth://totp/Peter:{}?secret={secret_b32}&issuer=Peter&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}",
        enc(email)
    )
}

/// Ten recovery codes like "7KQ2-M9XD-4RTP", easy to read and type.
pub fn new_recovery_codes() -> Vec<String> {
    // No 0/O or 1/I, which are easy to confuse.
    const CHARS: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
    (0..10)
        .map(|_| {
            // Bytes 6 and 8 of a v4 UUID carry fixed version bits; skip them.
            let id = uuid::Uuid::new_v4();
            let b = id.as_bytes();
            let random = [b[0], b[1], b[2], b[3], b[4], b[5], b[7], b[9], b[10], b[11], b[12], b[13]];
            // 32 characters divide 256 evenly, so every character is equally likely.
            let chars: String = random.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect();
            format!("{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12])
        })
        .collect()
}

/// Recovery codes are compared without dashes, spaces or case.
pub fn normalize_recovery(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_test_vectors() {
        // RFC 6238 appendix B, SHA-1, 8 digits.
        let secret = b"12345678901234567890";
        for (time, expected) in [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(code_at(secret, time / STEP_SECS, 8), expected, "time {time}");
        }
    }

    #[test]
    fn base32_round_trip_and_rfc4648_vectors() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_decode("MZXW6YTBOI======").unwrap(), b"foobar");
        let s = new_secret();
        assert_eq!(s.len(), 32);
        assert_eq!(base32_decode(&s).unwrap().len(), 20);
    }

    #[test]
    fn verify_accepts_drift_and_refuses_reuse() {
        let secret = new_secret();
        let raw = base32_decode(&secret).unwrap();
        let now = 1_800_000_000;
        let step = now / STEP_SECS;
        let code = code_at(&raw, step, DIGITS);
        assert_eq!(verify(&secret, &code, now, 0), Some(step));
        assert_eq!(verify(&secret, &format!("{} {}", &code[..3], &code[3..]), now, 0), Some(step));
        // A code from 30 seconds ago still works once; a used step does not.
        let previous = code_at(&raw, step - 1, DIGITS);
        assert_eq!(verify(&secret, &previous, now, 0), Some(step - 1));
        assert_eq!(verify(&secret, &code, now, step), None);
        // Too old, or not a code.
        assert_eq!(verify(&secret, &code_at(&raw, step - 3, DIGITS), now, 0), None);
        assert_eq!(verify(&secret, "12345", now, 0), None);
    }

    #[test]
    fn recovery_codes_look_right() {
        let codes = new_recovery_codes();
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|c| c.len() == 14 && c.matches('-').count() == 2));
        assert_eq!(normalize_recovery("7kq2 m9xd-4rtp"), "7KQ2M9XD4RTP");
        assert!(otpauth_url("ABC", "ana+qa@example.com").starts_with("otpauth://totp/Peter:ana%2Bqa%40example.com?secret=ABC&issuer=Peter"));
    }
}
