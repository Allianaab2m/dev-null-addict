use std::fmt;
use std::path::{Path, PathBuf};
use std::{io, time::SystemTimeError};

/// The ed25519 key this machine signs its requests with.
///
/// The user approves it once as a device of their account. It is never a key
/// on their GitHub account. The file is an unencrypted OpenSSH private key,
/// like the one the mobile app keeps in its keychain.
#[derive(Clone)]
pub struct DeviceKey {
    key: ssh_key::PrivateKey,
}

impl DeviceKey {
    /// `$XDG_DATA_HOME/dev-null-addict/key`, or `~/.local/share/dev-null-addict/key`
    /// when `XDG_DATA_HOME` is unset, on Linux and macOS alike.
    pub fn default_path() -> Result<PathBuf, KeyError> {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|s| !s.is_empty())
                    .map(|p| PathBuf::from(p).join(".local/share"))
            })
            .map(|p| p.join("dev-null-addict/key"))
            .ok_or(KeyError::NoHome)
    }

    /// Reads the key, and refuses it if its group or others may read it, as
    /// ssh does.
    pub fn load(path: &Path) -> Result<Self, KeyError> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                KeyError::Missing(path.to_owned())
            } else {
                key_io(path, source)
            }
        })?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(KeyError::TooOpen {
                path: path.to_owned(),
                mode,
            });
        }
        check_directory(path)?;
        let text = std::fs::read_to_string(path).map_err(|e| key_io(path, e))?;
        let key = ssh_key::PrivateKey::from_openssh(text).map_err(|source| KeyError::Format {
            path: path.to_owned(),
            source,
        })?;
        if key.is_encrypted() {
            return Err(KeyError::Encrypted(path.to_owned()));
        }
        if key.algorithm() != ssh_key::Algorithm::Ed25519 {
            return Err(KeyError::NotEd25519(path.to_owned()));
        }
        Ok(Self { key })
    }

    /// Makes a new key and writes it to `path` with mode 0600, creating its
    /// directory with mode 0700. Fails if a file is already there.
    pub fn generate(path: &Path, comment: &str) -> Result<Self, KeyError> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| key_io(parent, e))?;
        }
        check_directory(path)?;
        let mut key = ssh_key::PrivateKey::random(
            &mut ssh_key::rand_core::OsRng,
            ssh_key::Algorithm::Ed25519,
        )
        .map_err(|source| KeyError::Format {
            path: path.to_owned(),
            source,
        })?;
        key.set_comment(comment);
        let encoded =
            key.to_openssh(ssh_key::LineEnding::LF)
                .map_err(|source| KeyError::Format {
                    path: path.to_owned(),
                    source,
                })?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| key_io(path, e))?;
        file.write_all(encoded.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| key_io(path, e))?;
        Ok(Self { key })
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint(self.key.fingerprint(ssh_key::HashAlg::Sha256).to_string())
    }

    /// Signs a request as /dev/null checks it.
    ///
    /// The signed message is the timestamp, a fresh nonce, `method`,
    /// `path_and_query` and `body`, joined by `\n`, signed like
    /// `ssh-keygen -Y sign -n devnull-api`. `path_and_query` has to be what
    /// goes on the request line, byte for byte, and `body` the exact bytes
    /// sent (empty without a body).
    pub fn sign_request(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
    ) -> Result<SignedHeaders, SignError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
            .to_string();
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)?;
        let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let message = signed_message(&timestamp, &nonce, method, path_and_query, body);
        let sig = self
            .key
            .sign("devnull-api", ssh_key::HashAlg::Sha512, &message)?;
        let pem = sig.to_pem(ssh_key::LineEnding::LF)?;
        let signature = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        Ok(SignedHeaders {
            timestamp,
            nonce,
            signature,
        })
    }
}

/// Shows the fingerprint, never the key.
impl fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceKey")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

/// A key's SHA256 fingerprint, `SHA256:` and unpadded base64, as
/// `ssh-keygen -l` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The headers that make a request signed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SignedHeaders {
    /// Unix time in seconds. /dev/null accepts it within five minutes of its
    /// own clock.
    pub timestamp: String,
    /// 32 hex digits, good for one request.
    pub nonce: String,
    /// The SSHSIG signature, base64 on one line.
    pub signature: String,
}

impl SignedHeaders {
    pub const TIMESTAMP: &str = "X-Devnull-Timestamp";
    pub const NONCE: &str = "X-Devnull-Nonce";
    pub const SIGNATURE: &str = "X-SSH-Signature";
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("there is no device key at {}", .0.display())]
    Missing(PathBuf),
    #[error("{} may be read by others (mode {mode:03o}); use mode 0600 for the key and 0700 for its directory", path.display())]
    TooOpen { path: PathBuf, mode: u32 },
    #[error("{} is not an ed25519 key", .0.display())]
    NotEd25519(PathBuf),
    #[error("{} is encrypted; the device key has to be unencrypted", .0.display())]
    Encrypted(PathBuf),
    #[error("could not read or write {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read the key in {}", path.display())]
    Format {
        path: PathBuf,
        #[source]
        source: ssh_key::Error,
    },
    #[error("neither XDG_DATA_HOME nor HOME is set")]
    NoHome,
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("signing failed")]
    Signature(#[from] ssh_key::Error),
    #[error("no random numbers for the nonce")]
    Random(#[from] getrandom::Error),
    #[error("the clock is before 1970")]
    Clock(#[from] SystemTimeError),
}

fn key_io(path: &Path, source: io::Error) -> KeyError {
    KeyError::Io {
        path: path.to_owned(),
        source,
    }
}

fn check_directory(path: &Path) -> Result<(), KeyError> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mode = std::fs::metadata(parent)
            .map_err(|e| key_io(parent, e))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(KeyError::TooOpen {
                path: parent.to_owned(),
                mode,
            });
        }
    }
    Ok(())
}

fn signed_message(timestamp: &str, nonce: &str, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let mut message = format!("{timestamp}\n{nonce}\n{method}\n{path}\n").into_bytes();
    message.extend_from_slice(body);
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    // Verify the actual SSHSIG header against the exact request bytes, including escaping and query order.
    #[test]
    fn signed_headers_verify_with_ssh_key() {
        let dir = TestDir::new();
        let key = DeviceKey::generate(&dir.0.join("key"), "test device").unwrap();
        let body = br#"{"body":"hello\nworld"}"#;
        let path = "/api/v1/log/latest?cursor=a%2Bb%2F%3D";
        let headers = key.sign_request("POST", path, body).unwrap();
        assert_eq!(headers.nonce.len(), 32);
        assert!(headers.nonce.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(headers.timestamp.parse::<u64>().is_ok());
        assert!(!headers.signature.contains(char::is_whitespace));
        let wrapped = headers
            .signature
            .as_bytes()
            .chunks(70)
            .map(|part| std::str::from_utf8(part).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let pem =
            format!("-----BEGIN SSH SIGNATURE-----\n{wrapped}\n-----END SSH SIGNATURE-----\n");
        let signature = ssh_key::SshSig::from_pem(pem).unwrap();
        let mut message =
            format!("{}\n{}\nPOST\n{path}\n", headers.timestamp, headers.nonce).into_bytes();
        message.extend_from_slice(body);
        key.key
            .public_key()
            .verify("devnull-api", &message, &signature)
            .unwrap();
        assert!(
            key.key
                .public_key()
                .verify("wrong-namespace", &message, &signature)
                .is_err()
        );
        message.push(b' ');
        assert!(
            key.key
                .public_key()
                .verify("devnull-api", &message, &signature)
                .is_err()
        );
        assert_ne!(
            headers.nonce,
            key.sign_request("POST", path, body).unwrap().nonce
        );
        assert_eq!(SignedHeaders::TIMESTAMP, "X-Devnull-Timestamp");
        assert_eq!(SignedHeaders::NONCE, "X-Devnull-Nonce");
        assert_eq!(SignedHeaders::SIGNATURE, "X-SSH-Signature");
    }

    // Keys must never overwrite another device or become usable with exposed file or directory permissions.
    #[test]
    fn key_permissions_and_debug_are_safe() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new();
        let path = dir.0.join("device/key");
        let key = DeviceKey::generate(&path, "secret comment").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            DeviceKey::load(&path).unwrap().fingerprint(),
            key.fingerprint()
        );
        assert!(DeviceKey::generate(&path, "replacement").is_err());
        let debug = format!("{key:?}");
        assert!(debug.contains(key.fingerprint().as_str()));
        assert!(!debug.contains("PRIVATE KEY"));
        assert!(!debug.contains("secret comment"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            DeviceKey::load(&path),
            Err(KeyError::TooOpen { .. })
        ));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(matches!(
            DeviceKey::load(&path),
            Err(KeyError::TooOpen { .. })
        ));
    }
}
