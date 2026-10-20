//! Approving this machine's key as a device, the way the mobile app signs in.
//!
//! The app opens `/mobile/authorize` with a PKCE challenge, the user approves,
//! and the page sends a one-time code back to the app's `devnull://` URL. A
//! terminal cannot receive that URL, so the user copies it from the
//! `Location` header of the approval in the browser's developer tools, and
//! [`Authorization::accept`] reads it.
//!
//! The code expires after five minutes, works once, and only together with
//! this key and the PKCE verifier that never leaves this machine. Unlike a
//! session cookie it is no use to anyone else.

use std::fmt;

use url::Url;

use crate::api::{BaseUrl, DeviceApproval, DeviceKey};

/// A device approval that waits for the user in the browser.
#[derive(Clone)]
pub struct Authorization {
    url: Url,
    state: String,
    code_verifier: String,
}

impl Authorization {
    /// Where /dev/null is told to send the code. It has to start with
    /// `devnull://`.
    pub const REDIRECT_URI: &str = "devnull://dev-null-addict/callback";

    /// Makes a PKCE verifier (S256) and a state, and the approval page's URL
    /// for `key` under the name `device_name`, which the user sees on the
    /// page and in Settings, Devices.
    pub fn new(
        base: &BaseUrl,
        key: &DeviceKey,
        device_name: &str,
    ) -> Result<Self, getrandom::Error> {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use sha2::{Digest, Sha256};
        let mut random = [0u8; 48];
        getrandom::fill(&mut random)?;
        let code_verifier = URL_SAFE_NO_PAD.encode(&random[..32]);
        let state = URL_SAFE_NO_PAD.encode(&random[32..]);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
        let mut url = base
            .as_url()
            .join("/mobile/authorize")
            .expect("base URL is hierarchical");
        url.query_pairs_mut().extend_pairs([
            ("fingerprint", key.fingerprint().as_str()),
            ("device_name", device_name),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("redirect_uri", Self::REDIRECT_URI),
        ]);
        Ok(Self {
            url,
            state,
            code_verifier,
        })
    }

    /// The page to open in the browser.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Reads what the user copied: the `devnull://` URL from the approval's
    /// `Location` header, with or without `Location:` in front and with any
    /// surrounding whitespace. Checks that it answers this authorization.
    pub fn accept(&self, pasted: &str) -> Result<DeviceApproval, CallbackError> {
        let pasted = pasted.trim();
        let pasted = pasted
            .get(..9)
            .filter(|p| p.eq_ignore_ascii_case("location:"))
            .map_or(pasted, |_| pasted[9..].trim());
        let url = Url::parse(pasted).map_err(|_| CallbackError::Malformed)?;
        let expected = Url::parse(Self::REDIRECT_URI).expect("constant URL");
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.path() != expected.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err(CallbackError::Malformed);
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        let get = |name: &str| -> Result<Option<&str>, CallbackError> {
            let values: Vec<_> = pairs
                .iter()
                .filter(|(k, _)| k == name)
                .map(|(_, v)| v.as_ref())
                .collect();
            match values.as_slice() {
                [] => Ok(None),
                [value] => Ok(Some(value)),
                _ => Err(CallbackError::Malformed),
            }
        };
        if get("state")? != Some(self.state.as_str()) {
            return Err(CallbackError::StateMismatch);
        }
        if get("error")?.is_some() {
            return Err(CallbackError::Denied);
        }
        let code = get("code")?
            .filter(|code| !code.is_empty())
            .ok_or(CallbackError::Malformed)?;
        Ok(DeviceApproval {
            code: code.to_owned(),
            code_verifier: self.code_verifier.clone(),
        })
    }
}

/// Shows the URL, leaves out the verifier.
impl fmt::Debug for Authorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authorization")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum CallbackError {
    #[error("that is not the devnull:// address of an approval")]
    Malformed,
    #[error("the approval was cancelled in the browser")]
    Denied,
    #[error("that approval belongs to another sign-in; start again")]
    StateMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    // PKCE uses S256 and the callback is bound to this state and exact redirect endpoint.
    #[test]
    fn pkce_and_callback_validation() {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use sha2::{Digest, Sha256};
        let dir = crate::test_support::TestDir::new();
        let key = DeviceKey::generate(&dir.0.join("key"), "test").unwrap();
        let auth = Authorization::new(&BaseUrl::default(), &key, "work station").unwrap();
        let pairs: std::collections::HashMap<_, _> = auth.url().query_pairs().collect();
        assert_eq!(
            pairs["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(auth.code_verifier.as_bytes()))
        );
        assert_eq!(pairs["fingerprint"], key.fingerprint().as_str());
        assert_eq!(pairs["redirect_uri"], Authorization::REDIRECT_URI);
        assert_eq!(pairs["device_name"], "work station");
        let callback = format!(
            "{}?code=one%2Btime&state={}",
            Authorization::REDIRECT_URI,
            auth.state
        );
        let approval = auth.accept(&format!("  Location: {callback}\r\n")).unwrap();
        assert_eq!(approval.code, "one+time");
        assert_eq!(approval.code_verifier, auth.code_verifier);
        assert_eq!(
            auth.accept(&format!("{callback}&state={}", auth.state)),
            Err(CallbackError::Malformed)
        );
        assert_eq!(
            auth.accept(&format!("{callback}&code=second")),
            Err(CallbackError::Malformed)
        );
        assert_eq!(
            auth.accept(&callback.replace("/callback", "/wrong")),
            Err(CallbackError::Malformed)
        );
        assert_eq!(
            auth.accept(&callback.replace(&auth.state, "wrong")),
            Err(CallbackError::StateMismatch)
        );
        assert_eq!(
            auth.accept(&format!(
                "{}?error=access_denied&state={}",
                Authorization::REDIRECT_URI,
                auth.state
            )),
            Err(CallbackError::Denied)
        );
        assert_eq!(
            auth.accept(&format!(
                "{}?code=&state={}",
                Authorization::REDIRECT_URI,
                auth.state
            )),
            Err(CallbackError::Malformed)
        );
        assert!(!format!("{auth:?}").contains(&auth.code_verifier));
        assert!(!format!("{approval:?}").contains("one+time"));
        assert!(!format!("{approval:?}").contains(&auth.code_verifier));
    }
}
