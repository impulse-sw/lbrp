use impulse_ui_kit::router::endpoint;
use impulse_utils::prelude::*;
use lbrp_cli_authorize::{CBAChallengeSign, LbrpAuthorize, TokenBundle};
use lbrp_types::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse};

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
    .await
    .map_err(CliError::from)?
    .error_for_status()
    .map_err(CliError::from)?;

  let state = resp
    .headers()
    .get(lbrp_cli_authorize::SIGNUP_HINTS)
    .ok_or(CliError::from_str("Ошибка сервера"))?
    .to_str()
    .map_err(|e| CliError::from(e))?
    .to_string();

  let resp = resp.json::<RegisterResponse>().await.map_err(CliError::from)?;

  Ok((state, resp.challenge.unwrap()))
}

pub(crate) async fn sign_up_step2(
  id: String,
  password: String,
  state: String,
  cdpub: Vec<u8>,
  cba_challenge_sign: CBAChallengeSign,
) -> CResult<TokenBundle> {
  let triple = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-up-step2"))
    .header(lbrp_cli_authorize::SIGNUP_HINTS, state)
    .json(&RegisterRequest {
      id,
      password,
      cdpub: Some(cdpub),
      cba_challenge_sign: Some(cba_challenge_sign),
    })
    .send()
    .await
    .map_err(CliError::from)?
    .error_for_status()
    .map_err(CliError::from)?
    .json::<TokenBundle>()
    .await
    .map_err(CliError::from)?;

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
    .await
    .map_err(CliError::from)?
    .error_for_status()
    .map_err(CliError::from)?
    .json::<LoginResponse>()
    .await
    .map_err(CliError::from)?;

  Ok(resp.challenge.unwrap())
}

pub(crate) async fn login_step2(
  id: String,
  password: String,
  cdpub: Vec<u8>,
  cba_challenge_sign: CBAChallengeSign,
) -> CResult<TokenBundle> {
  let triple = reqwest::Client::new()
    .post(endpoint("/--inner-lbrp-auth/sign-in-step2"))
    .json(&LoginRequest {
      id,
      password,
      cdpub: Some(cdpub),
      cba_challenge_sign: Some(cba_challenge_sign),
    })
    .send()
    .await
    .map_err(CliError::from)?
    .error_for_status()
    .map_err(CliError::from)?
    .json::<TokenBundle>()
    .await
    .map_err(CliError::from)?;

  Ok(triple)
}

pub(crate) async fn check_auth() -> bool {
  reqwest::Client::new()
    .get("/")
    .lbrp_authorize(endpoint("/--inner-lbrp-auth/checkup"))
    .await
    .is_ok()
}
