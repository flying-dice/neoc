//! Minting secrets: the one place the host turns random bytes into a token or an id.

use crate::ports::RandomSource;
use agentos_domain::hex_of;

/// `bytes` random bytes from `random`, as lowercase hex. Session tokens, device ids and agent tokens are all
/// minted with this.
#[must_use]
pub fn random_hex(random: &dyn RandomSource, bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    random.fill(&mut buf);
    hex_of(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::SeededRandom;

    #[test]
    fn a_token_is_two_hex_digits_per_byte_and_never_repeats() {
        let random = SeededRandom::default();
        let (a, b) = (random_hex(&random, 24), random_hex(&random, 24));
        assert_eq!(a.len(), 48);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
    }
}
