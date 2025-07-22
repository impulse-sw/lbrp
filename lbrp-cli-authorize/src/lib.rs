//! Client SDK for LBRP.
//!
//! This crate contains functions to work with client signatures and token persistance.

#![deny(warnings, clippy::todo, clippy::unimplemented)]
#![feature(let_chains)]

use impulse_utils::errors::{CliError, ErrorResponse};
use impulse_utils::results::CResult;

pub use authnz_common::SIGNUP_HINTS;
pub use authnz_common::{CBAChallengeSign, Email, SignKeypair, TokenBundle};

pub(crate) mod utils;

const LBRP_CBA_CERT: &str = "__lbrp_client_keypair";

/// Gets or generates client-side keypair.
pub fn client_keypair() -> CResult<SignKeypair> {
  if let Some(cert) = crate::utils::get_from_storage(LBRP_CBA_CERT)
    && let Ok(keypair) = SignKeypair::unpack_keypair(cert)
  {
    Ok(keypair)
  } else {
    Ok(generate_and_save())
  }
}

fn generate_and_save() -> SignKeypair {
  let keypair = SignKeypair::new_ed25519().unwrap();
  crate::utils::put_in_storage(LBRP_CBA_CERT, &keypair.pack_keypair());
  keypair
}

#[allow(async_fn_in_trait)]
pub trait LbrpAuthorize
where
  Self: Sized,
{
  async fn lbrp_authorize(self, endpoint: impl AsRef<str>) -> CResult<Self>;
}

pub trait ClientPlatformAware {
  fn include_creds(self) -> Self;
}

impl ClientPlatformAware for reqwest::RequestBuilder {
  fn include_creds(self) -> Self {
    #[cfg(target_arch = "wasm32")]
    {
      self.fetch_credentials_include()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
      self
    }
  }
}

fn extract_and_decode_header(resp: &reqwest::Response, header_name: impl AsRef<str>) -> Option<Vec<u8>> {
  resp.headers().get(header_name.as_ref()).and_then(|encoded| {
    encoded
      .to_str()
      .ok()
      .and_then(|str_encoded| authnz_common::base64_decode(str_encoded).ok())
  })
}

fn extract_header(resp: &reqwest::Response, header_name: impl AsRef<str>) -> Option<&str> {
  resp
    .headers()
    .get(header_name.as_ref())
    .and_then(|header_val| header_val.to_str().ok())
}

fn auth_err_handler(builder: reqwest::RequestBuilder, bytes: &[u8]) -> CResult<reqwest::RequestBuilder> {
  if let Ok(authorize_response) = serde_json::from_slice::<authnz_common::ApplicationAuthorizeResponse>(bytes)
    && authorize_response.authorized
  {
    Ok(builder.include_creds())
  } else if let Ok(err_resp) = serde_json::from_slice::<ErrorResponse>(bytes) {
    Err(CliError::from_str(err_resp.err))
  } else {
    Err(CliError::from_str(format!(
      "Unknown error: `{:?}`",
      String::from_utf8_lossy(bytes)
    )))
  }
}

impl LbrpAuthorize for reqwest::RequestBuilder {
  /// Automatically gets token if persisted.
  async fn lbrp_authorize(self, endpoint: impl AsRef<str>) -> CResult<Self> {
    let resp = reqwest::Client::new()
      .post(endpoint.as_ref())
      .include_creds()
      .send()
      .await
      .map_err(CliError::from)?;

    if let Some(challenge) = extract_and_decode_header(&resp, lbrp_types::LBRP_CHALLENGE)
      && let Some(challenge_state) = extract_header(&resp, lbrp_types::LBRP_CHALLENGE_STATE)
    {
      let keypair = client_keypair()?;
      let sign = keypair.sign_raw(&challenge);

      let resp2 = reqwest::Client::new()
        .post(endpoint.as_ref())
        .include_creds()
        .header(lbrp_types::LBRP_CHALLENGE_STATE, challenge_state)
        .header(lbrp_types::LBRP_CHALLENGE_SIGN, authnz_common::base64_encode(&sign))
        .send()
        .await
        .map_err(CliError::from)?
        .bytes()
        .await
        .map_err(CliError::from)?;

      return auth_err_handler(self, &resp2);
    }

    auth_err_handler(self, resp.bytes().await.map_err(CliError::from)?.as_ref())
  }
}
