use std::str::FromStr;
use std::sync::Arc;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use async_trait::async_trait;
use chrono::Utc;
use homemagic_domain::{Actor, ActorGrant, ActorId, InstallationId};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{ActorCredential, BoxError, CommandRepository};

const TOKEN_PREFIX: &str = "hm1";
const SECRET_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

/// One-time bearer token that zeroizes its allocation on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ActorToken(String);

impl ActorToken {
    /// Exposes the token only for one-time operator delivery or authentication.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ActorToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ActorToken([REDACTED])")
    }
}

/// Generic authentication failure that does not reveal actor existence or state.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("invalid actor credentials")]
pub struct ActorAuthenticationError;

/// Actor bootstrap, rotation, disable, or grant-management failure.
#[derive(Debug, Error)]
pub enum ActorManagementError {
    /// Durable actor state could not be read or written.
    #[error("actor repository operation failed")]
    Repository(#[source] BoxError),
    /// Requested actor does not exist.
    #[error("actor not found")]
    NotFound,
    /// Password-hash generation failed without exposing token material.
    #[error("actor credential hashing failed")]
    Hashing,
}

/// Application-owned actor token bootstrap and verification service.
#[derive(Clone)]
pub struct ActorAuthentication {
    repository: Arc<dyn CommandRepository>,
}

impl ActorAuthentication {
    /// Creates authentication against the durable command repository.
    #[must_use]
    pub fn new(repository: Arc<dyn CommandRepository>) -> Self {
        Self { repository }
    }

    /// Creates an enabled actor and returns its random bearer token exactly once.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe management error when hashing or persistence fails.
    pub async fn bootstrap(
        &self,
        installation_id: InstallationId,
        name: impl Into<String>,
    ) -> Result<(Actor, ActorToken), ActorManagementError> {
        let actor = Actor {
            id: ActorId::new(),
            installation_id,
            name: name.into(),
            enabled: true,
            created_at: Utc::now(),
        };
        let (token, credential) = issue_credential(&actor).await?;
        self.repository
            .store_actor(actor.clone(), Some(credential))
            .await
            .map_err(ActorManagementError::Repository)?;
        Ok((actor, token))
    }

    /// Rotates an actor credential and returns the replacement token once.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` or a secret-safe hashing/repository failure.
    pub async fn rotate(&self, actor_id: &ActorId) -> Result<ActorToken, ActorManagementError> {
        let security = self
            .repository
            .actor_security(actor_id)
            .await
            .map_err(ActorManagementError::Repository)?
            .ok_or(ActorManagementError::NotFound)?;
        let (token, credential) = issue_credential(&security.actor).await?;
        self.repository
            .store_actor(security.actor, Some(credential))
            .await
            .map_err(ActorManagementError::Repository)?;
        Ok(token)
    }

    /// Disables an actor without deleting credentials or audit identity.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` or a secret-safe repository failure.
    pub async fn disable(&self, actor_id: &ActorId) -> Result<(), ActorManagementError> {
        let mut security = self
            .repository
            .actor_security(actor_id)
            .await
            .map_err(ActorManagementError::Repository)?
            .ok_or(ActorManagementError::NotFound)?;
        security.actor.enabled = false;
        self.repository
            .store_actor(security.actor, None)
            .await
            .map_err(ActorManagementError::Repository)
    }

    /// Atomically replaces explicit narrow policy grants for an actor.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe repository failure.
    pub async fn replace_grants(
        &self,
        actor_id: &ActorId,
        grants: Vec<ActorGrant>,
    ) -> Result<(), ActorManagementError> {
        self.repository
            .replace_actor_grants(actor_id, grants)
            .await
            .map_err(ActorManagementError::Repository)
    }

    /// Authenticates one complete bearer value away from async worker threads.
    ///
    /// # Errors
    ///
    /// Always returns the same generic error for malformed, unknown, disabled,
    /// missing, or incorrect credentials.
    pub async fn authenticate(&self, bearer: &str) -> Result<Actor, ActorAuthenticationError> {
        let parsed = parse_token(bearer)?;
        let security = self
            .repository
            .actor_security(&parsed.actor_id)
            .await
            .map_err(|_| ActorAuthenticationError)?
            .ok_or(ActorAuthenticationError)?;
        if !security.actor.enabled {
            return Err(ActorAuthenticationError);
        }
        let credential = security.credential.ok_or(ActorAuthenticationError)?;
        let secret = parsed.secret;
        let verified = tokio::task::spawn_blocking(move || verify(&secret, &credential.token_hash))
            .await
            .map_err(|_| ActorAuthenticationError)?;
        if verified {
            Ok(security.actor)
        } else {
            Err(ActorAuthenticationError)
        }
    }
}

struct ParsedToken {
    actor_id: ActorId,
    secret: Zeroizing<String>,
}

fn parse_token(value: &str) -> Result<ParsedToken, ActorAuthenticationError> {
    let mut parts = value.split('.');
    let prefix = parts.next().ok_or(ActorAuthenticationError)?;
    let actor = parts.next().ok_or(ActorAuthenticationError)?;
    let secret = parts.next().ok_or(ActorAuthenticationError)?;
    if prefix != TOKEN_PREFIX
        || parts.next().is_some()
        || secret.len() != SECRET_BYTES * 2
        || !secret.bytes().all(|byte| byte.is_ascii_hexdigit())
        || secret.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(ActorAuthenticationError);
    }
    Ok(ParsedToken {
        actor_id: ActorId::from_str(actor).map_err(|_| ActorAuthenticationError)?,
        secret: Zeroizing::new(secret.to_owned()),
    })
}

async fn issue_credential(
    actor: &Actor,
) -> Result<(ActorToken, ActorCredential), ActorManagementError> {
    let secret = rand::random::<[u8; SECRET_BYTES]>();
    let salt = rand::random::<[u8; SALT_BYTES]>();
    let secret = Zeroizing::new(hex(&secret));
    let token = ActorToken(format!("{TOKEN_PREFIX}.{}.{}", actor.id, secret.as_str()));
    let hash_secret = secret.clone();
    let token_hash = tokio::task::spawn_blocking(move || hash(&hash_secret, &salt))
        .await
        .map_err(|_| ActorManagementError::Hashing)??;
    Ok((
        token,
        ActorCredential {
            actor_id: actor.id.clone(),
            token_hash,
            rotated_at: Utc::now(),
        },
    ))
}

fn hash(secret: &str, salt: &[u8]) -> Result<String, ActorManagementError> {
    let salt = SaltString::encode_b64(salt).map_err(|_| ActorManagementError::Hashing)?;
    argon2()
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ActorManagementError::Hashing)
}

fn verify(secret: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded)
        .ok()
        .is_some_and(|hash| argon2().verify_password(secret.as_bytes(), &hash).is_ok())
}

fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

/// Transport-neutral authentication boundary used by HTTP and WebSocket APIs.
#[async_trait]
pub trait AuthenticateActor: Send + Sync {
    /// Resolves one bearer value to its non-spoofable durable actor.
    async fn authenticate_actor(&self, bearer: &str) -> Result<Actor, ActorAuthenticationError>;
}

#[async_trait]
impl AuthenticateActor for ActorAuthentication {
    async fn authenticate_actor(&self, bearer: &str) -> Result<Actor, ActorAuthenticationError> {
        self.authenticate(bearer).await
    }
}
