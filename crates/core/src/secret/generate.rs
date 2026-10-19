//! Passwords the daemon generates for a secret field (AC-6).

use stackly_platform::{MAX_SECRET_LEN, SecretValue, SecretValueError};

/// The length of a generated password before it is clamped to the
/// `minLength` and `maxLength` of its secret field (spec 0009).
pub const GENERATED_LEN: usize = 32;

/// The characters of a generated password, `[A-Za-z0-9]`.
const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
/// Random bytes at or above this are dropped, so every character of
/// [`ALPHABET`] is equally likely: 248 is the largest multiple of 62 that
/// fits a byte.
const ACCEPT_BELOW: u8 = 248;
/// How many random bytes are drawn at a time.
const BATCH: usize = 64;

/// Error returned by [`generate_password`]. It never holds a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GenerateError {
    /// The OS random source failed.
    #[error("the OS random source failed: {reason}")]
    Random {
        /// What the random source reported.
        reason: String,
    },
    /// The length cannot hold a secret value: zero, or more than
    /// [`MAX_SECRET_LEN`](stackly_platform::MAX_SECRET_LEN) bytes.
    #[error("a generated password cannot be {len} characters long: {source}")]
    Length {
        /// The length asked for.
        len: usize,
        /// The rule of [`SecretValue`] the length breaks.
        source: SecretValueError,
    },
}

/// Generates a password of `len` characters from `[A-Za-z0-9]`, drawn from
/// the OS random source with no bias toward any character.
///
/// # Errors
///
/// Returns [`GenerateError::Length`] for a length of zero or above
/// [`MAX_SECRET_LEN`](stackly_platform::MAX_SECRET_LEN), and
/// [`GenerateError::Random`] when the OS random source fails.
pub fn generate_password(len: usize) -> Result<SecretValue, GenerateError> {
    let too_short_or_long = match len {
        0 => Some(SecretValueError::Empty),
        _ if len > MAX_SECRET_LEN => Some(SecretValueError::TooLong),
        _ => None,
    };
    if let Some(source) = too_short_or_long {
        return Err(GenerateError::Length { len, source });
    }
    // The capacity is final, so the password is never copied by a
    // reallocation before `SecretValue` takes it.
    let mut password = String::with_capacity(len);
    let mut random = [0_u8; BATCH];
    while password.len() < len {
        getrandom::fill(&mut random).map_err(|err| GenerateError::Random {
            reason: err.to_string(),
        })?;
        for &byte in random.iter().filter(|&&byte| byte < ACCEPT_BELOW) {
            if password.len() == len {
                break;
            }
            password.push(char::from(ALPHABET[usize::from(byte) % ALPHABET.len()]));
        }
    }
    random.fill(0);
    std::hint::black_box(&random);
    SecretValue::new(password).map_err(|source| GenerateError::Length { len, source })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn two_thousand_passwords_are_distinct_and_use_the_whole_alphabet_only() {
        let mut passwords = BTreeSet::new();
        let mut characters = BTreeSet::new();
        for _ in 0..2000 {
            let password = generate_password(GENERATED_LEN).unwrap();
            let text = password.expose();
            assert_eq!(text.len(), 32);
            assert!(text.bytes().all(|byte| ALPHABET.contains(&byte)), "{text}");
            characters.extend(text.bytes());
            passwords.insert(text.to_owned());
        }

        assert_eq!(passwords.len(), 2000);
        assert_eq!(characters.len(), ALPHABET.len());
    }

    #[test]
    fn a_length_must_fit_a_secret_value() {
        assert_eq!(generate_password(1).unwrap().len(), 1);
        assert_eq!(
            generate_password(MAX_SECRET_LEN).unwrap().len(),
            MAX_SECRET_LEN
        );
        assert_eq!(
            generate_password(0).unwrap_err(),
            GenerateError::Length {
                len: 0,
                source: SecretValueError::Empty
            }
        );
        assert_eq!(
            generate_password(MAX_SECRET_LEN + 1).unwrap_err(),
            GenerateError::Length {
                len: MAX_SECRET_LEN + 1,
                source: SecretValueError::TooLong
            }
        );
        assert_eq!(
            generate_password(usize::MAX).unwrap_err().to_string(),
            format!(
                "a generated password cannot be {} characters long: a secret value must be at most 65536 bytes",
                usize::MAX
            )
        );
    }
}
