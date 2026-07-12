//! Persistent actor authentication and token-redaction canaries.

use std::error::Error;
use std::sync::Arc;

use chrono::Utc;
use homemagic_application::{
    ActorAuthentication, CommandRepository, FoundationRepository, FoundationWrite,
};
use homemagic_domain::{ActorKind, Installation, InstallationId};
use homemagic_storage::SqliteRepository;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn token_should_authenticate_rotate_disable_and_never_persist_raw() -> TestResult {
    let directory = tempfile::tempdir()?;
    let repository = Arc::new(SqliteRepository::open(
        directory.path().join("authentication.sqlite3"),
    )?);
    let installation_id = InstallationId::new();
    repository
        .apply(FoundationWrite {
            installations: vec![Installation {
                id: installation_id.clone(),
                name: "Home".to_owned(),
                created_at: Utc::now(),
            }],
            ..FoundationWrite::default()
        })
        .await?;
    let authentication = ActorAuthentication::new(repository.clone());
    let (actor, token) = authentication
        .bootstrap_principal(installation_id, "Automation agent", ActorKind::Agent)
        .await?;
    let token_value = token.expose().to_owned();
    let security = repository
        .actor_security(&actor.id)
        .await?
        .ok_or("bootstrapped actor missing")?;
    let stored_hash = security
        .credential
        .ok_or("bootstrapped credential missing")?
        .token_hash;

    assert_eq!(authentication.authenticate(&token_value).await?, actor);
    assert!(stored_hash.starts_with("$argon2id$"));
    assert!(!stored_hash.contains(&token_value));
    assert_eq!(format!("{token:?}"), "ActorToken([REDACTED])");

    let mut wrong = token_value.clone();
    let replacement = if wrong.ends_with('0') { "1" } else { "0" };
    wrong.replace_range(wrong.len() - 1.., replacement);
    assert!(authentication.authenticate(&wrong).await.is_err());

    let rotated = authentication.rotate(&actor.id).await?;
    assert!(authentication.authenticate(&token_value).await.is_err());
    assert_eq!(authentication.authenticate(rotated.expose()).await?, actor);

    authentication.disable(&actor.id).await?;
    assert!(authentication.authenticate(rotated.expose()).await.is_err());
    Ok(())
}
