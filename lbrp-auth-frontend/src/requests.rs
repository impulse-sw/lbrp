use cc_utils::prelude::*;
use lbrp_cli_authorize::LbrpAuthorize;
use lbrp_types::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse};

pub(crate) fn endpoint(api_uri: impl AsRef<str>) -> String {
  use cc_ui_kit::router::{get_host, get_protocol};

  format!(
    "{}//{}{}",
    get_protocol().unwrap(),
    get_host().unwrap(),
    api_uri.as_ref()
  )
}

pub(crate) async fn sign_up_step1(id: String) -> CResult<(String, Vec<u8>)> {
  let resp = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-up-step1"))
    .json(&RegisterRequest {
      id,
      password: String::new(),
      cdpub: None,
      cba_challenge_sign: None,
    })
    .send()
    .await?
    .error_for_status()?;

  let state = resp
    .headers()
    .get(lbrp_cli_authorize::PREREGISTER_HEADER)
    .ok_or(CliError::from("Ошибка сервера"))?
    .to_str()
    .map_err(|_| CliError::from("Ошибка сервера"))?
    .to_string();

  let resp = resp.json::<RegisterResponse>().await?;

  Ok((state, resp.challenge.unwrap()))
}

pub(crate) async fn sign_up_step2(
  id: String,
  password: String,
  state: String,
  cdpub: Vec<u8>,
  cba_challenge_sign: Vec<u8>,
) -> CResult<lbrp_cli_authorize::TokenTriple> {
  let triple = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-up-step2"))
    .header(lbrp_cli_authorize::PREREGISTER_HEADER, state)
    .json(&RegisterRequest {
      id,
      password,
      cdpub: Some(cdpub),
      cba_challenge_sign: Some(cba_challenge_sign),
    })
    .send()
    .await?
    .error_for_status()?
    .json::<lbrp_cli_authorize::TokenTriple>()
    .await?;

  Ok(triple)
}

pub(crate) async fn login_step1(id: String) -> CResult<Vec<u8>> {
  let resp = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-in-step1"))
    .json(&LoginRequest {
      id,
      password: String::new(),
      cdpub: None,
      cba_challenge_sign: None,
    })
    .send()
    .await?
    .error_for_status()?
    .json::<LoginResponse>()
    .await?;

  Ok(resp.challenge.unwrap())
}

pub(crate) async fn login_step2(
  id: String,
  password: String,
  cdpub: Vec<u8>,
  cba_challenge_sign: Vec<u8>,
) -> CResult<lbrp_cli_authorize::TokenTriple> {
  let triple = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-in-step2"))
    .json(&LoginRequest {
      id,
      password,
      cdpub: Some(cdpub),
      cba_challenge_sign: Some(cba_challenge_sign),
    })
    .send()
    .await?
    .error_for_status()?
    .json::<lbrp_cli_authorize::TokenTriple>()
    .await?;

  Ok(triple)
}

pub(crate) async fn check_auth() -> bool {
  reqwest::Client::new()
    .get("/")
    .lbrp_authorize(endpoint("/--inner-lbrp-auth/checkup"))
    .await
    .is_ok()
}
