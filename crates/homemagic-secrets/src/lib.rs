//! Secret-store adapters for platform services and explicit headless operation.

use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, Key, KeyInit, Payload},
};
use homemagic_application::{SecretStore, SecretStoreError, SecretValue};
use homemagic_domain::SecretRef;
use serde::{Deserialize, Serialize};
use tokio::fs;
use zeroize::Zeroizing;

const FILE_BACKEND: &str = "encrypted-file";
const ENVELOPE_VERSION: u8 = 1;

/// Platform keychain adapter backed by macOS Keychain or Linux Secret Service.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, Debug)]
pub struct PlatformSecretStore {
    service: String,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl PlatformSecretStore {
    /// Creates a platform adapter under the given application service name.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(service: &str, reference: &SecretRef) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new(service, reference.as_str())
            .map_err(|_| platform_error("open", "entry_unavailable"))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[async_trait]
impl SecretStore for PlatformSecretStore {
    fn backend(&self) -> &'static str {
        "platform"
    }

    async fn put(&self, reference: &SecretRef, value: SecretValue) -> Result<(), SecretStoreError> {
        let service = self.service.clone();
        let reference = reference.clone();
        tokio::task::spawn_blocking(move || {
            let value = std::str::from_utf8(value.expose())
                .map_err(|_| platform_error("put", "invalid_utf8"))?;
            Self::entry(&service, &reference)?
                .set_password(value)
                .map_err(|_| platform_error("put", "backend_failure"))
        })
        .await
        .map_err(|_| platform_error("put", "worker_failure"))?
    }

    async fn get(&self, reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
        let service = self.service.clone();
        let reference = reference.clone();
        tokio::task::spawn_blocking(move || {
            Self::entry(&service, &reference)?
                .get_password()
                .map(|value| SecretValue::new(value.into_bytes()))
                .map_err(|error| match error {
                    keyring::Error::NoEntry => platform_error("get", "not_found"),
                    _ => platform_error("get", "backend_failure"),
                })
        })
        .await
        .map_err(|_| platform_error("get", "worker_failure"))?
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), SecretStoreError> {
        let service = self.service.clone();
        let reference = reference.clone();
        tokio::task::spawn_blocking(move || {
            Self::entry(&service, &reference)?
                .delete_credential()
                .map_err(|error| match error {
                    keyring::Error::NoEntry => platform_error("delete", "not_found"),
                    _ => platform_error("delete", "backend_failure"),
                })
        })
        .await
        .map_err(|_| platform_error("delete", "worker_failure"))?
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const fn platform_error(operation: &'static str, code: &'static str) -> SecretStoreError {
    SecretStoreError {
        backend: "platform",
        operation,
        code,
    }
}

/// Explicit headless encrypted-file vault.
#[derive(Clone, Debug)]
pub struct FileSecretStore {
    directory: PathBuf,
    key_file: PathBuf,
}

impl FileSecretStore {
    /// Validates and creates a file-vault adapter.
    ///
    /// # Errors
    ///
    /// Refuses keys with invalid length, unsafe Unix permissions, or a key
    /// file located inside the vault data directory.
    pub async fn open(
        directory: impl Into<PathBuf>,
        key_file: impl Into<PathBuf>,
    ) -> Result<Self, SecretStoreError> {
        let store = Self {
            directory: directory.into(),
            key_file: key_file.into(),
        };
        store.validate_key_file().await?;
        fs::create_dir_all(&store.directory)
            .await
            .map_err(|_| file_error("open", "data_directory_unavailable"))?;
        Ok(store)
    }

    fn secret_path(&self, reference: &SecretRef) -> PathBuf {
        self.directory.join(format!("{}.json", reference.as_str()))
    }

    async fn validate_key_file(&self) -> Result<(), SecretStoreError> {
        let key_canonical = fs::canonicalize(&self.key_file)
            .await
            .map_err(|_| file_error("open", "key_unavailable"))?;
        let data_canonical = canonical_parent(&self.directory)
            .await
            .map_err(|_| file_error("open", "data_directory_unavailable"))?;
        if key_canonical.starts_with(&data_canonical) {
            return Err(file_error("open", "key_inside_data_directory"));
        }
        let metadata = fs::metadata(&key_canonical)
            .await
            .map_err(|_| file_error("open", "key_unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 {
                return Err(file_error("open", "unsafe_key_permissions"));
            }
            if metadata.uid() != rustix::process::getuid().as_raw() {
                return Err(file_error("open", "wrong_key_owner"));
            }
        }
        let key = fs::read(&key_canonical)
            .await
            .map_err(|_| file_error("open", "key_unavailable"))?;
        if key.len() != 32 {
            return Err(file_error("open", "invalid_key_length"));
        }
        Ok(())
    }

    async fn key(&self, operation: &'static str) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
        let key = Zeroizing::new(
            fs::read(&self.key_file)
                .await
                .map_err(|_| file_error(operation, "key_unavailable"))?,
        );
        if key.len() != 32 {
            return Err(file_error(operation, "invalid_key_length"));
        }
        Ok(key)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Envelope {
    version: u8,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[async_trait]
impl SecretStore for FileSecretStore {
    fn backend(&self) -> &'static str {
        FILE_BACKEND
    }

    async fn put(&self, reference: &SecretRef, value: SecretValue) -> Result<(), SecretStoreError> {
        let key = self.key("put").await?;
        let key = Key::<XChaCha20Poly1305>::try_from(key.as_slice())
            .map_err(|_| file_error("put", "invalid_key_length"))?;
        let cipher = XChaCha20Poly1305::new(&key);
        let nonce = XNonce::generate();
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: value.expose(),
                    aad: reference.as_str().as_bytes(),
                },
            )
            .map_err(|_| file_error("put", "encryption_failed"))?;
        let payload = serde_json::to_vec(&Envelope {
            version: ENVELOPE_VERSION,
            nonce: nonce.to_vec(),
            ciphertext,
        })
        .map_err(|_| file_error("put", "encoding_failed"))?;
        let target = self.secret_path(reference);
        let temporary = target.with_extension("json.tmp");
        fs::write(&temporary, payload)
            .await
            .map_err(|_| file_error("put", "write_failed"))?;
        fs::rename(&temporary, &target)
            .await
            .map_err(|_| file_error("put", "commit_failed"))?;
        Ok(())
    }

    async fn get(&self, reference: &SecretRef) -> Result<SecretValue, SecretStoreError> {
        let payload = fs::read(self.secret_path(reference))
            .await
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    file_error("get", "not_found")
                } else {
                    file_error("get", "read_failed")
                }
            })?;
        let envelope: Envelope =
            serde_json::from_slice(&payload).map_err(|_| file_error("get", "invalid_envelope"))?;
        if envelope.version != ENVELOPE_VERSION || envelope.nonce.len() != 24 {
            return Err(file_error("get", "unsupported_envelope"));
        }
        let key = self.key("get").await?;
        let key = Key::<XChaCha20Poly1305>::try_from(key.as_slice())
            .map_err(|_| file_error("get", "invalid_key_length"))?;
        let cipher = XChaCha20Poly1305::new(&key);
        let nonce = XNonce::try_from(envelope.nonce.as_slice())
            .map_err(|_| file_error("get", "unsupported_envelope"))?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &envelope.ciphertext,
                    aad: reference.as_str().as_bytes(),
                },
            )
            .map(SecretValue::new)
            .map_err(|_| file_error("get", "decryption_failed"))
    }

    async fn delete(&self, reference: &SecretRef) -> Result<(), SecretStoreError> {
        fs::remove_file(self.secret_path(reference))
            .await
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    file_error("delete", "not_found")
                } else {
                    file_error("delete", "delete_failed")
                }
            })
    }
}

async fn canonical_parent(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path).await
    } else if let Some(parent) = path.parent() {
        fs::canonicalize(parent).await.map(|value| {
            path.file_name()
                .map_or(value.clone(), |name| value.join(name))
        })
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "missing parent"))
    }
}

const fn file_error(operation: &'static str, code: &'static str) -> SecretStoreError {
    SecretStoreError {
        backend: FILE_BACKEND,
        operation,
        code,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    async fn fixture() -> (tempfile::TempDir, FileSecretStore) {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("temporary root: {error}"));
        let key = root.path().join("master.key");
        fs::write(&key, [7_u8; 32])
            .await
            .unwrap_or_else(|error| panic!("write key: {error}"));
        fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))
            .await
            .unwrap_or_else(|error| panic!("secure key: {error}"));
        let store = FileSecretStore::open(root.path().join("vault"), &key)
            .await
            .unwrap_or_else(|error| panic!("open vault: {error}"));
        (root, store)
    }

    #[tokio::test]
    async fn vault_should_round_trip_without_plaintext_on_disk() {
        let (_root, store) = fixture().await;
        let reference = SecretRef::from_backend_id("fixture");
        let canary = b"secret-canary-do-not-persist";

        store
            .put(&reference, SecretValue::new(canary.to_vec()))
            .await
            .unwrap_or_else(|error| panic!("put secret: {error}"));
        let encrypted = fs::read(store.secret_path(&reference))
            .await
            .unwrap_or_else(|error| panic!("read envelope: {error}"));
        assert!(
            !encrypted
                .windows(canary.len())
                .any(|window| window == canary)
        );
        let resolved = store
            .get(&reference)
            .await
            .unwrap_or_else(|error| panic!("get secret: {error}"));
        assert_eq!(resolved.expose(), canary);
    }

    #[tokio::test]
    async fn vault_should_bind_ciphertext_to_reference() {
        let (_root, store) = fixture().await;
        let first = SecretRef::from_backend_id("first");
        let second = SecretRef::from_backend_id("second");
        store
            .put(&first, SecretValue::new(b"canary".to_vec()))
            .await
            .unwrap_or_else(|error| panic!("put secret: {error}"));
        fs::copy(store.secret_path(&first), store.secret_path(&second))
            .await
            .unwrap_or_else(|error| panic!("copy envelope: {error}"));

        let error = store.get(&second).await.err();
        assert_eq!(error.map(|value| value.code), Some("decryption_failed"));
    }

    #[tokio::test]
    async fn vault_should_reject_permissive_master_key() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("temporary root: {error}"));
        let key = root.path().join("master.key");
        fs::write(&key, [7_u8; 32])
            .await
            .unwrap_or_else(|error| panic!("write key: {error}"));
        fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644))
            .await
            .unwrap_or_else(|error| panic!("set permissions: {error}"));

        let error = FileSecretStore::open(root.path().join("vault"), &key)
            .await
            .err();
        assert_eq!(
            error.map(|value| value.code),
            Some("unsafe_key_permissions")
        );
    }

    #[test]
    fn secret_debug_output_should_be_redacted() {
        assert_eq!(
            format!("{:?}", SecretValue::new(b"credential-canary".to_vec())),
            "SecretValue([REDACTED])"
        );
    }
}
