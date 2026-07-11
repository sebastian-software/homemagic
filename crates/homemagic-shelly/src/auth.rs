use reqwest::header::HeaderValue;
use sha2::{Digest, Sha256};
use thiserror::Error;

const USERNAME: &str = "admin";
const ALGORITHM: &str = "SHA-256";
const QOP: &str = "auth";

pub(crate) struct DigestChallenge {
    realm: String,
    nonce: String,
    stale: bool,
}

impl std::fmt::Debug for DigestChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DigestChallenge")
            .field("realm", &self.realm)
            .field("nonce", &"[REDACTED]")
            .field("stale", &self.stale)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum DigestError {
    #[error("authentication challenge is missing")]
    Missing,
    #[error("authentication challenge uses an unsupported scheme")]
    UnsupportedScheme,
    #[error("authentication challenge is malformed")]
    Malformed,
    #[error("authentication challenge uses an unsupported algorithm")]
    UnsupportedAlgorithm,
    #[error("authentication challenge uses an unsupported quality of protection")]
    UnsupportedQop,
    #[error("credential is not valid UTF-8")]
    InvalidCredential,
    #[error("authorization header could not be encoded")]
    InvalidHeader,
}

impl DigestError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Missing => "challenge_missing",
            Self::UnsupportedScheme => "unsupported_scheme",
            Self::Malformed => "challenge_malformed",
            Self::UnsupportedAlgorithm => "unsupported_algorithm",
            Self::UnsupportedQop => "unsupported_qop",
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidHeader => "invalid_header",
        }
    }
}

impl DigestChallenge {
    pub(crate) fn parse(value: Option<&HeaderValue>) -> Result<Self, DigestError> {
        let value = value.ok_or(DigestError::Missing)?;
        let value = value.to_str().map_err(|_| DigestError::Malformed)?;
        let Some(parameters) = value.strip_prefix("Digest ") else {
            return Err(DigestError::UnsupportedScheme);
        };
        let mut realm = None;
        let mut nonce = None;
        let mut algorithm = None;
        let mut qop = None;
        let mut stale = false;

        for parameter in split_parameters(parameters)? {
            let Some((key, value)) = parameter.split_once('=') else {
                return Err(DigestError::Malformed);
            };
            let value = value.trim().trim_matches('"');
            match key.trim().to_ascii_lowercase().as_str() {
                "realm" => realm = Some(value.to_owned()),
                "nonce" => nonce = Some(value.to_owned()),
                "algorithm" => algorithm = Some(value.to_owned()),
                "qop" => qop = Some(value.to_owned()),
                "stale" => stale = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }

        if !algorithm
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(ALGORITHM))
        {
            return Err(DigestError::UnsupportedAlgorithm);
        }
        if !qop
            .as_deref()
            .is_some_and(|value| value.split(',').any(|item| item.trim() == QOP))
        {
            return Err(DigestError::UnsupportedQop);
        }
        let realm = realm
            .filter(|value| !value.is_empty())
            .ok_or(DigestError::Malformed)?;
        let nonce = nonce
            .filter(|value| !value.is_empty())
            .ok_or(DigestError::Malformed)?;
        Ok(Self {
            realm,
            nonce,
            stale,
        })
    }

    pub(crate) const fn stale(&self) -> bool {
        self.stale
    }

    pub(crate) fn authorization(
        &self,
        password: &[u8],
        method: &str,
        uri: &str,
        nonce_count: u32,
        client_nonce: u32,
    ) -> Result<HeaderValue, DigestError> {
        let password = std::str::from_utf8(password).map_err(|_| DigestError::InvalidCredential)?;
        let nonce_count = format!("{nonce_count:08x}");
        let ha1 = sha256_hex(format!("{USERNAME}:{}:{password}", self.realm));
        let ha2 = sha256_hex(format!("{method}:{uri}"));
        let response = sha256_hex(format!(
            "{ha1}:{}:{nonce_count}:{client_nonce}:{QOP}:{ha2}",
            self.nonce
        ));
        let value = format!(
            "Digest username=\"{USERNAME}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\", algorithm={ALGORITHM}, response=\"{response}\", qop={QOP}, nc={nonce_count}, cnonce=\"{client_nonce}\"",
            self.realm, self.nonce
        );
        HeaderValue::from_str(&value).map_err(|_| DigestError::InvalidHeader)
    }
}

fn split_parameters(value: &str) -> Result<Vec<&str>, DigestError> {
    let mut parameters = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, character) in value.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                parameters.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err(DigestError::Malformed);
    }
    parameters.push(value[start..].trim());
    Ok(parameters)
}

fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(value.as_ref());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_should_match_independent_shelly_formula_vector() {
        let challenge = DigestChallenge {
            realm: "shellyplus1-aabbccddeeff".to_owned(),
            nonce: "1625053638".to_owned(),
            stale: false,
        };

        let header = challenge
            .authorization(
                b"fixture-password",
                "GET",
                "/rpc/Shelly.GetStatus",
                1,
                313_273_957,
            )
            .unwrap_or_else(|error| panic!("authorization: {error}"));
        let header = header
            .to_str()
            .unwrap_or_else(|error| panic!("header text: {error}"));

        assert!(header.contains("nc=00000001"));
        assert!(header.contains("algorithm=SHA-256"));
        assert!(header.contains(
            "response=\"3db1ea2b1a2e9ed7249975d83ecf4c05ee3cd524f39c1695e0db08aeb1a56da0\""
        ));
        assert!(!header.contains("fixture-password"));
    }

    #[test]
    fn challenge_parser_should_support_modern_and_legacy_nonce_shapes() {
        for fixture in [
            include_str!("../tests/fixtures/auth_challenge_modern.txt"),
            include_str!("../tests/fixtures/auth_challenge_legacy.txt"),
        ] {
            let header = HeaderValue::from_str(fixture.trim())
                .unwrap_or_else(|error| panic!("fixture header: {error}"));
            let challenge = DigestChallenge::parse(Some(&header))
                .unwrap_or_else(|error| panic!("challenge: {error}"));
            assert!(!challenge.realm.is_empty());
            assert!(!challenge.nonce.is_empty());
        }
    }

    #[test]
    fn debug_output_should_redact_nonce() {
        let header = HeaderValue::from_static(
            "Digest qop=\"auth\", realm=\"fixture\", nonce=\"nonce-canary\", algorithm=SHA-256",
        );
        let challenge = DigestChallenge::parse(Some(&header))
            .unwrap_or_else(|error| panic!("challenge: {error}"));
        let output = format!("{challenge:?}");
        assert!(!output.contains("nonce-canary"));
        assert!(output.contains("[REDACTED]"));
    }
}
