//! Client SDK for LBRP.
//!
//! This crate contains functions to work with client signatures and token persistance.

#![deny(warnings, clippy::todo, clippy::unimplemented)]
#![feature(let_chains)]

use cc_utils::errors::{CliError, ErrorResponse};
use cc_utils::results::CResult;

pub use c3a_common::PREREGISTER_HEADER;
pub use c3a_common::{Email, Keypair, TokenTriple};
pub use c3a_common::{
  pack_cert, pack_into as pack_triple, sign_raw, unpack_cert, unpack_from as unpack_triple, verify_raw,
};

pub(crate) mod utils;

const LBRP_CBA_CERT: &str = "__lbrp_client_keypair";

/// Gets or generates client-side keypair.
pub fn client_keypair() -> CResult<Keypair> {
  if let Some(cert) = crate::utils::get_from_storage(LBRP_CBA_CERT)
    && let Ok(keypair) = c3a_common::unpack_cert(cert)
  {
    Ok(keypair)
  } else {
    Ok(generate_and_save())
  }
}

fn generate_and_save() -> Keypair {
  let keypair = c3a_common::generate_keypair();
  crate::utils::put_in_storage(LBRP_CBA_CERT, &c3a_common::pack_cert(&keypair));
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
      .and_then(|str_encoded| c3a_common::base64_decode(str_encoded).ok())
  })
}

fn extract_header(resp: &reqwest::Response, header_name: impl AsRef<str>) -> Option<&str> {
  resp
    .headers()
    .get(header_name.as_ref())
    .and_then(|header_val| header_val.to_str().ok())
}

fn auth_err_handler(builder: reqwest::RequestBuilder, bytes: &[u8]) -> CResult<reqwest::RequestBuilder> {
  if let Ok(authorize_response) = serde_json::from_slice::<c3a_common::AuthorizeResponse>(bytes)
    && authorize_response.authorized
  {
    Ok(builder.include_creds())
  } else if let Ok(err_resp) = serde_json::from_slice::<ErrorResponse>(bytes) {
    Err(err_resp.err.into())
  } else {
    Err(format!("Unknown error: `{:?}`", String::from_utf8_lossy(bytes)).into())
  }
}

impl LbrpAuthorize for reqwest::RequestBuilder {
  /// Automatically gets token if persisted.
  async fn lbrp_authorize(self, endpoint: impl AsRef<str>) -> CResult<Self> {
    let resp = reqwest::Client::new()
      .post(endpoint.as_ref())
      .include_creds()
      .send()
      .await?;

    if let Some(challenge) = extract_and_decode_header(&resp, lbrp_types::LBRP_CHALLENGE)
      && let Some(challenge_state) = extract_header(&resp, lbrp_types::LBRP_CHALLENGE_STATE)
    {
      let keypair = client_keypair()?;
      let sign = sign_raw(&challenge, &keypair);

      let resp2 = reqwest::Client::new()
        .post(endpoint.as_ref())
        .include_creds()
        .header(lbrp_types::LBRP_CHALLENGE_STATE, challenge_state)
        .header(lbrp_types::LBRP_CHALLENGE_SIGN, c3a_common::base64_encode(&sign))
        .send()
        .await
        .map_err(CliError::from)?
        .bytes()
        .await?;

      return auth_err_handler(self, &resp2);
    }

    auth_err_handler(self, resp.bytes().await?.as_ref())
  }
}
