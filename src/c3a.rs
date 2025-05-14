use c3a_server_sdk::{
  AuthorizeResponse, C3AClient,
  c3a_common::{self, AuthenticationStepApproval, TokenTriple},
};
use cc_server_kit::prelude::*;
use lbrp_types::{LoginRequest, LoginResponse, RegisterRequest, RegisterResponse};

pub(crate) async fn init_authcli(filepath: &str) -> MResult<C3AClient> {
  let (keypair, invite, mut opts) = c3a_server_sdk::persist_c3a_opts(
    filepath,
    c3a_common::AppAuthConfiguration {
      app_name: "lbrp".into(),
      allowed_tags: vec![
        ("user", "simple").into(),
        ("user", "restricted").into(),
        ("admin", "restricted").into(),
      ],
      sign_up_opts: c3a_common::SignUpOpts {
        identify_by: c3a_common::IdenticationRequirement::Nickname {
          spaces: false,
          upper_registry: false,
          characters: false,
        },
        allow_sign_up: false,
        auto_assign_tags: vec![("user", "simple").into()],
        allowed_authentication_flow: vec![c3a_common::AuthenticationRequirement::Password {
          min_size: 8,
          should_contain_different_case: false,
          should_contain_symbols: false,
        }],
        required_authentication: vec![c3a_common::AuthenticationRequirement::Password {
          min_size: 8,
          should_contain_different_case: false,
          should_contain_symbols: false,
        }],
      },
      sign_in_opts: c3a_common::SignInOpts {
        allow_honeypots: false,
        enable_fail_to_ban: None,
        allow_recovery_key: false,
        token_encryption_type: c3a_common::TokenEncryptionType::ChaCha20Poly1305,
      },
      client_based_auth_opts: c3a_common::ClientBasedAuthorizationOpts {
        enable_cba: true,
        enable_cba_private_gateway_by: None,
      },
      tokens_lifetime: Default::default(),
      author_dpub: vec![],
    },
  )
  .map_err(|e| ServerError::from_private(e).with_500())?;

  tracing::debug!("PUBLIC KEY: {:?}", keypair.verifying_key().as_bytes());

  let mut auth_cli = C3AClient::new(opts.app_name.as_str(), keypair, "http://127.0.0.1:19806")
    .await
    .map_err(|e| ServerError::from_private(e).with_500())?;

  if auth_cli.update_config(&opts).await.is_err() {
    tracing::info!("App is not registered! Registering...");
    auth_cli
      .app_register(invite, opts.clone())
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }
  tracing::info!("Registered & updated.");

  tracing::info!("Checking out for admin user...");

  let id = c3a_common::Id::Nickname {
    nickname: "archibald-host".into(),
  };
  if auth_cli.check_user_exists(id.clone()).await.is_err() {
    let ckeypair =
      c3a_common::unpack_cert(std::env::var("LBRP_C3A_ADMCDPUB").map_err(|e| ServerError::from_private(e).with_500())?)
        .map_err(|e| ServerError::from_private(e).with_500())?;
    let password = std::env::var("LBRP_C3A_ADMP").map_err(|e| ServerError::from_private(e).with_500())?;

    opts.sign_up_opts.allow_sign_up = true;
    auth_cli
      .update_config(&opts)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    let (prereg, state) = auth_cli
      .prepare_sign_up(id.clone())
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    let challenge_sign = c3a_common::sign_raw(&prereg.cba_challenge.unwrap(), &ckeypair);

    auth_cli
      .perform_sign_up(
        id.clone(),
        state,
        vec![vec![c3a_common::AuthenticationStepApproval::Password { password }]],
        Some(ckeypair.verifying_key().as_bytes().to_vec()),
        Some(challenge_sign),
      )
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    opts.sign_up_opts.allow_sign_up = false;
    auth_cli
      .update_config(&opts)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    if !auth_cli
      .get_user_tags(id.clone())
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?
      .iter()
      .any(|v| v.scope.as_str().eq("restricted"))
    {
      auth_cli
        .edit_user_tags(id, &[("admin", "restricted").into()], &[])
        .await
        .map_err(|e| ServerError::from_private(e).with_500())?;
    }
  }

  Ok(auth_cli)
}

pub(crate) struct MaybeC3ARedirect {
  pub(crate) tags: Vec<c3a_common::AppTag>,
}

impl MaybeC3ARedirect {
  pub(crate) fn new(tags: Vec<c3a_common::AppTag>) -> Self {
    Self { tags }
  }
}

#[cc_server_kit::salvo::async_trait]
impl cc_server_kit::salvo::Handler for MaybeC3ARedirect {
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {}
}

pub(crate) fn extract_authcli(depot: &Depot) -> MResult<&C3AClient> {
  depot
    .obtain::<C3AClient>()
    .map_err(|_| ServerError::from_private_str("Can't get auth client from depot!").with_500())
}

#[handler]
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
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
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
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

  C3AClient::deploy_triple_to_cookies(&triple, res);

  json!(triple)
}

#[handler]
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
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
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
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

  C3AClient::deploy_triple_to_cookies(&triple, res);

  json!(triple)
}

#[handler]
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
async fn check_auth(depot: &mut Depot, req: &mut Request, res: &mut Response) -> MResult<Json<AuthorizeResponse>> {
  let auth_cli = extract_authcli(depot)?;
  auth_cli.check_signed_in(req, res).await.and_then(|v| json!(v))
}

#[handler]
#[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
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
    .push(cc_static_server::frontend_router_from_given_dist(
      &std::path::PathBuf::from("lbrp-auth-frontend/dist/--inner-lbrp-auth"),
    ))
}
