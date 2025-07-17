use c3a_server_sdk::{
  ApplicationKeyring, AuthClient,
  c3a_common::{
    self, AppAuthConfiguration, AuthenticationApproval, AuthenticationFlow, AuthenticationFlows, CBAChallengeSign,
    ClientBasedAuthorizationOpts, GeneralAuthenticationRequirement, IdenticationRequirement, SignInOpts, SignUpOpts,
    TokenLifetimes,
  },
};
use cc_server_kit::prelude::*;

mod auth_router;
mod middleware;

pub(crate) use auth_router::auth_router;
pub(crate) use middleware::MaybeC3ARedirect;

pub(crate) async fn init_authcli() -> MResult<AuthClient> {
  let keyring = ApplicationKeyring::from_file("lbrp-keyring.json");
  let keyring = if let Ok(mut keyring) = keyring {
    if !keyring.has_keypair() {
      keyring.new_keypair()?;
      keyring.save("lbrp-keyring.json")?;
    }
    keyring
  } else {
    let r#default = ApplicationKeyring::new()?;
    r#default.save("lbrp-keyring.json")?;
    r#default
  };

  let config = AppAuthConfiguration::from_file("lbrp-authnz-config.json");
  let mut config = if let Ok(config) = config {
    config
  } else {
    let r#default = AppAuthConfiguration::new("lbrp")
      .with_tags(&[("user", "simple"), ("user", "restricted"), ("admin", "restricted")])
      .sign_up_opts(
        SignUpOpts::new()
          .with_auto_tags(&[("user", "simple")])
          .with_signup_enabled(false)
          .with_id_type(IdenticationRequirement::simple_nickname())
          .with_allowed_steps(&[GeneralAuthenticationRequirement::password(8, false, false)])
          .with_required_steps(&[GeneralAuthenticationRequirement::password(8, false, false)]),
      )
      .sign_in_opts(SignInOpts::default())
      .cba_opts(ClientBasedAuthorizationOpts::default())
      .lifetimes(TokenLifetimes::default())
      .build()?;
    r#default.save("lbrp-authnz-config.json")?;
    r#default
  };

  let mut auth_cli = AuthClient::new(&config.app_name, keyring.keypair()?, "http://127.0.0.1:19806").await?;
  auth_cli.change_cookie_names([
    lbrp_types::LBRP_ACCESS,
    lbrp_types::LBRP_REFRESH,
    lbrp_types::LBRP_CLIENT,
  ]);
  auth_cli.change_challenge_header_names([
    lbrp_types::LBRP_CHALLENGE,
    lbrp_types::LBRP_CHALLENGE_STATE,
    lbrp_types::LBRP_CHALLENGE_SIGN,
  ]);

  if auth_cli.update_config(&config).await.is_err() {
    tracing::info!("App is not registered! Registering...");
    auth_cli
      .app_register(keyring.invite()?, config.clone())
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }
  tracing::info!("Registered & updated.");

  tracing::info!("Checking out for admin user...");

  let id = c3a_common::Id::Nickname {
    nickname: "archibald-host".into(),
  };
  if auth_cli.check_user_exists(id.clone()).await.is_err() {
    let ckeypair = c3a_common::SignKeypair::unpack_keypair(
      std::env::var("LBRP_C3A_ADMCDPUB").map_err(|e| ServerError::from_private(e).with_500())?,
    )
    .map_err(|e| ServerError::from_private(e).with_500())?;
    let password = std::env::var("LBRP_C3A_ADMP").map_err(|e| ServerError::from_private(e).with_500())?;

    config.sign_up_opts.allow_sign_up = true;
    auth_cli
      .update_config(&config)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    let (prereg, state) = auth_cli
      .prepare_sign_up(id.clone())
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    let challenge_sign = CBAChallengeSign::new(ckeypair.sign_raw(&prereg.cba_challenge.unwrap()));

    auth_cli
      .perform_sign_up(
        id.clone(),
        state,
        AuthenticationFlows::new().with(AuthenticationFlow::new().with(AuthenticationApproval::password(password))),
        Some(ckeypair.public()),
        Some(challenge_sign),
      )
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    config.sign_up_opts.allow_sign_up = false;
    auth_cli
      .update_config(&config)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }

  let tags = auth_cli
    .get_user_tags(id.clone())
    .await
    .map_err(|e| ServerError::from_private(e).with_500())?;

  tracing::info!("Admin tags: {:?}", tags);

  if !tags.iter().any(|v| v.scope().eq("restricted")) {
    auth_cli
      .edit_user_tags(id, &[("admin", "restricted").into()], &[])
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }

  Ok(auth_cli)
}

pub(crate) fn extract_authcli(depot: &Depot) -> MResult<&AuthClient> {
  depot
    .obtain::<AuthClient>()
    .map_err(|_| ServerError::from_private_str("Can't get auth client from depot!").with_500())
}
