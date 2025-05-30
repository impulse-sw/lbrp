use c3a_server_sdk::{C3AClient, c3a_common};
use cc_server_kit::prelude::*;

mod auth_client;
mod auth_router;
mod middleware;

pub(crate) use auth_router::auth_router;
pub(crate) use middleware::MaybeC3ARedirect;

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

  tracing::debug!("PUBLIC KEY: {:?}", keypair.public());

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
    let ckeypair = c3a_common::SignKeypair::unpack_keypair(
      std::env::var("LBRP_C3A_ADMCDPUB").map_err(|e| ServerError::from_private(e).with_500())?,
    )
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

    let challenge_sign = ckeypair.sign_raw(&prereg.cba_challenge.unwrap());

    auth_cli
      .perform_sign_up(
        id.clone(),
        state,
        vec![vec![c3a_common::AuthenticationStepApproval::Password { password }]],
        Some(ckeypair.public()),
        Some(challenge_sign),
      )
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    opts.sign_up_opts.allow_sign_up = false;
    auth_cli
      .update_config(&opts)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }

  let tags = auth_cli
    .get_user_tags(id.clone())
    .await
    .map_err(|e| ServerError::from_private(e).with_500())?;

  tracing::info!("Admin tags: {:?}", tags);

  if !tags.iter().any(|v| v.scope.as_str().eq("restricted")) {
    auth_cli
      .edit_user_tags(id, &[("admin", "restricted").into()], &[])
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;
  }

  Ok(auth_cli)
}

pub(crate) fn extract_authcli(depot: &Depot) -> MResult<&C3AClient> {
  depot
    .obtain::<C3AClient>()
    .map_err(|_| ServerError::from_private_str("Can't get auth client from depot!").with_500())
}
