//! Random values, and the hashes they are stored as.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the operating system's random source works");
    bytes
}

/// 32 random bytes as base64url: a session, ticket or challenge.
pub fn token() -> String {
    URL_SAFE_NO_PAD.encode(random::<32>())
}

/// A uniformly random code from 000000 to 999999.
pub fn six_digits() -> String {
    // Redraw the top sliver of u32 so every code is equally likely.
    const LIMIT: u32 = u32::MAX - u32::MAX % 1_000_000;
    loop {
        let n = u32::from_be_bytes(random());
        if n < LIMIT {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

/// How a sign-in code is stored: tied to its address, so one hash can't be tried against
/// another address's row.
pub fn code_hash(email: &str, code: &str) -> Vec<u8> {
    sha256(format!("{email}\n{code}").as_bytes())
}

/// Compares in time that doesn't depend on where the bytes first differ.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..100 {
            let code = six_digits();
            assert_eq!(code.len(), 6);
            assert!(code.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(token().len(), 43);
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
    }
}
