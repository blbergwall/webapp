/// `bytes` random bytes from the OS RNG, hex-encoded (so `2 * bytes` chars).
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random number generator failed");
    buf.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_charset() {
        let token = random_hex(32);
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn values_differ() {
        assert_ne!(random_hex(32), random_hex(32));
    }
}
