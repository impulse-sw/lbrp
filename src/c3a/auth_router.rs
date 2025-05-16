use c3a_server_sdk::{
  AuthorizeResponse,
  c3a_common::{self, AuthenticationStepApproval, TokenTriple},
};
use cc_server_kit::prelude::*;
use lbrp_types::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse};

use crate::c3a::{auth_client::LbrpAuthMethods, extract_authcli};

#[handler]
#[instrument(skip_all)]
async fn sign_up_step1(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<RegisterResponse>> {
  let query = req.parse_json::<RegisterRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid register request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let (requirements, state) = auth_cli
    .prepare_sign_up(c3a_common::Id::Nickname { nickname: query.id })
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the first step of a registration!")
        .with_500()
    })?;
  res
    .add_header(c3a_common::PREREGISTER_HEADER, state, true)
    .map_err(|e| ServerError::from_private(e).with_500())?;

  json!(RegisterResponse {
    challenge: requirements.cba_challenge,
  })
}

#[handler]
#[instrument(skip_all)]
async fn sign_up_step2(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<TokenTriple>> {
  let query = req.parse_json::<RegisterRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid register request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let state = req
    .headers()
    .get(c3a_common::PREREGISTER_HEADER)
    .ok_or(ServerError::from_public("No preregistration state!").with_401())?
    .to_str()
    .map_err(|e| ServerError::from_private(e).with_500())?
    .to_string();

  let triple = auth_cli
    .perform_sign_up(
      c3a_common::Id::Nickname { nickname: query.id },
      state,
      vec![vec![AuthenticationStepApproval::Password {
        password: query.password,
      }]],
      query.cdpub,
      query.cba_challenge_sign,
    )
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the second step of a registration!")
        .with_500()
    })?;

  <c3a_server_sdk::C3AClient as LbrpAuthMethods>::deploy_triple_to_cookies(&triple, res);

  json!(triple)
}

#[handler]
#[instrument(skip_all)]
async fn login_step1(depot: &mut Depot, req: &mut Request) -> MResult<Json<LoginResponse>> {
  let query = req.parse_json::<LoginRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid login request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let resp = auth_cli
    .prepare_login(c3a_common::Id::Nickname { nickname: query.id })
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the first step of signing in!")
        .with_500()
    })?;

  json!(LoginResponse {
    challenge: resp.cba_challenge,
  })
}

#[handler]
#[instrument(skip_all)]
async fn login_step2(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<TokenTriple>> {
  let query = req.parse_json::<LoginRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid login request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let triple = auth_cli
    .perform_login(
      c3a_common::Id::Nickname { nickname: query.id },
      vec![AuthenticationStepApproval::Password {
        password: query.password,
      }],
      query.cdpub,
      query.cba_challenge_sign,
    )
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the second step of signing in!")
        .with_500()
    })?;

  <c3a_server_sdk::C3AClient as LbrpAuthMethods>::deploy_triple_to_cookies(&triple, res);

  json!(triple)
}

#[handler]
#[allow(clippy::bind_instead_of_map)]
async fn check_auth(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<AuthorizeResponse>> {
  let auth_cli = extract_authcli(depot)?;
  <c3a_server_sdk::C3AClient as LbrpAuthMethods>::check_signed_in(auth_cli, req, res)
    .await
    .and_then(|v| json!(v))
}

#[handler]
async fn request_client_token(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<OK> {
  let auth_cli = extract_authcli(depot)?;
  <c3a_server_sdk::C3AClient as LbrpAuthMethods>::update_client_token(auth_cli, req, res).await
}

pub(crate) fn auth_router() -> Router {
  Router::with_path("/--inner-lbrp-auth")
    .push(Router::with_path("/sign-up-step1").post(sign_up_step1))
    .push(Router::with_path("/sign-up-step2").post(sign_up_step2))
    .push(Router::with_path("/sign-in-step1").post(login_step1))
    .push(Router::with_path("/sign-in-step2").post(login_step2))
    .push(Router::with_path("/checkup").post(check_auth))
    .push(Router::with_path("/revalidate").post(request_client_token))
    .push(cc_static_server::frontend_router_from_given_dist(
      &std::path::PathBuf::from("lbrp-auth-frontend/dist/--inner-lbrp-auth"),
    ))
}
