use authnz_server_sdk::authnz_common::{
  self, ApplicationAuthorizeResponse, AuthenticationApproval, AuthenticationFlow, AuthenticationFlows, TokenBundle,
};
use impulse_server_kit::prelude::*;
use lbrp_types::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse};

use crate::authnz::extract_authcli;

#[handler]
#[tracing::instrument(skip_all)]
async fn sign_up_step1(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<RegisterResponse>> {
  let query = req.parse_json_simd::<RegisterRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid register request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let (requirements, state) = auth_cli
    .prepare_sign_up(authnz_common::Id::Nickname { nickname: query.id })
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the first step of a registration!")
        .with_401()
    })?;
  res
    .add_header(authnz_common::SIGNUP_HINTS, state, true)
    .map_err(|e| ServerError::from_private(e).with_500())?;

  json!(RegisterResponse {
    challenge: requirements.cba_challenge,
  })
}

#[handler]
#[tracing::instrument(skip_all)]
async fn sign_up_step2(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<TokenBundle>> {
  let query = req.parse_json_simd::<RegisterRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid register request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let state = req
    .headers()
    .get(authnz_common::SIGNUP_HINTS)
    .ok_or(ServerError::from_public("No preregistration state!").with_401())?
    .to_str()
    .map_err(|e| ServerError::from_private(e).with_401())?
    .to_string();

  let triple = auth_cli
    .perform_sign_up(
      authnz_common::Id::Nickname { nickname: query.id },
      state,
      AuthenticationFlows::new().with(AuthenticationFlow::new().with(AuthenticationApproval::password(query.password))),
      query.cdpub,
      query.cba_challenge_sign,
    )
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the second step of a registration!")
        .with_401()
    })?;

  auth_cli.deploy_triple_to_cookies(&triple, res)?;

  json!(triple)
}

#[handler]
#[tracing::instrument(skip_all)]
async fn login_step1(depot: &mut Depot, req: &mut Request) -> MResult<Json<LoginResponse>> {
  let query = req.parse_json_simd::<LoginRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid login request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let resp = auth_cli
    .prepare_login(authnz_common::Id::Nickname { nickname: query.id })
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the first step of signing in!")
        .with_401()
    })?;

  json!(LoginResponse {
    challenge: resp.cba_challenge,
  })
}

#[handler]
#[tracing::instrument(skip_all)]
async fn login_step2(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<TokenBundle>> {
  let query = req.parse_json_simd::<LoginRequest>().await.map_err(|e| {
    ServerError::from_private(e)
      .with_public("Invalid login request!")
      .with_401()
  })?;
  let auth_cli = extract_authcli(depot)?;

  let triple = auth_cli
    .perform_login(
      authnz_common::Id::Nickname { nickname: query.id },
      AuthenticationFlow::new().with(AuthenticationApproval::password(query.password)),
      query.cdpub,
      query.cba_challenge_sign,
    )
    .await
    .map_err(|e| {
      ServerError::from_private(e)
        .with_private_str("Failed to perform the second step of signing in!")
        .with_401()
    })?;

  auth_cli.deploy_triple_to_cookies(&triple, res)?;

  json!(triple)
}

#[handler]
#[allow(clippy::bind_instead_of_map)]
async fn check_auth(
  depot: &mut Depot,
  req: &mut Request,
  res: &mut Response,
) -> MResult<Json<ApplicationAuthorizeResponse>> {
  let auth_cli = extract_authcli(depot)?;
  auth_cli.check_signed_in(req, res).await.and_then(|v| json!(v))
}

#[handler]
async fn request_client_token(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<OK> {
  let auth_cli = extract_authcli(depot)?;
  auth_cli.update_client_token(req, res).await
}

pub(crate) fn auth_router() -> Router {
  Router::with_path("/--inner-lbrp-auth")
    .push(Router::with_path("/sign-up-step1").post(sign_up_step1))
    .push(Router::with_path("/sign-up-step2").post(sign_up_step2))
    .push(Router::with_path("/sign-in-step1").post(login_step1))
    .push(Router::with_path("/sign-in-step2").post(login_step2))
    .push(Router::with_path("/checkup").post(check_auth))
    .push(Router::with_path("/revalidate").post(request_client_token))
    .push(
      impulse_static_server::frontend_router_from_given_dist(&std::path::PathBuf::from(
        "lbrp-auth-frontend/dist/--inner-lbrp-auth",
      ))
      .unwrap(),
    )
}
