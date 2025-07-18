#![deny(warnings, clippy::todo, clippy::unimplemented)]
#![feature(let_chains, string_from_utf8_lossy_owned, stmt_expr_attributes)]

#[cfg(feature = "c3a")]
mod c3a;
mod config;
mod cors_handling;
mod error_handling;
mod proxy_client;
mod router;

use c3a::init_authcli;
use cc_server_kit::cc_utils::prelude::*;
use cc_server_kit::prelude::*;
use cc_server_kit::salvo::affix_state;
use cc_server_kit::salvo::server::ServerHandle;
use cc_server_kit::setup::StartupVariant;
use cc_server_kit::startup::{get_root_router_autoinject, start_force_https_redirect, start_with_service};
use mimalloc::MiMalloc;
use serde::Deserialize;
use std::time::Duration;
use tokio::select;
use tokio::sync::broadcast;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use crate::config::{LbrpConfig, Service, config_watcher};
use crate::error_handling::ErrHandler;
use crate::router::get_router_from_config;

#[derive(Deserialize, Default, Clone)]
struct Setup {
  #[serde(flatten)]
  generic_values: GenericValues,
  config_file: Option<String>,
}

impl GenericSetup for Setup {
  fn generic_values(&self) -> &GenericValues {
    &self.generic_values
  }
  fn generic_values_mut(&mut self) -> &mut GenericValues {
    &mut self.generic_values
  }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> MResult<()> {
  let (reload_tx, _) = broadcast::channel::<()>(16);

  let setup = load_generic_config::<Setup>("lbrp-service").await.unwrap();
  let state = load_generic_state(&setup, true).await.unwrap();
  let mut children = vec![];

  loop {
    let watcher_tx = reload_tx.clone();

    let config_file = setup.config_file.as_deref().unwrap_or("lbrp-config.json").to_owned();
    let watcher_handle = tokio::spawn(async move {
      if let Err(e) = config_watcher(config_file, watcher_tx).await {
        eprintln!("Config watcher error: {e:?}");
      }
    });

    let mut reload_rx = reload_tx.subscribe();

    let file = std::fs::File::open(setup.config_file.as_deref().unwrap_or("lbrp-config.json")).map_err(|e| {
      ServerError::from_private(e)
        .with_public("Can't open `lbrp-config.json`!")
        .with_500()
    })?;
    let reader = std::io::BufReader::new(file);
    let config = match serde_json::from_reader::<_, LbrpConfig>(reader) {
      Ok(config) => {
        if let Err(e) = config.validate() {
          tracing::info!("Can't get the config due to: {}!", e);
          std::process::exit(1);
        }
        config
      }
      Err(e) => {
        tracing::info!("Can't get the config due to: {}!", e);
        std::process::exit(1);
      }
    };

    let lbrp_router = get_root_router_autoinject(&state, setup.clone())
      .hoop(affix_state::inject(init_authcli().await?))
      .push(get_router_from_config(&config, &mut children).await);

    tracing::info!("Router:\n{:?}", lbrp_router);

    let mut lbrp_service = salvo::Service::new(lbrp_router);

    if config.services.iter().any(|s| matches!(s, Service::ErrorHandler(_))) {
      lbrp_service = lbrp_service.catcher(salvo::catcher::Catcher::new(ErrHandler::new(
        crate::router::excluded_from_err_handling(&config.services),
      )));
    }

    if matches!(state.startup_variant, StartupVariant::HttpsOnly)
      || matches!(state.startup_variant, StartupVariant::Quinn)
      || matches!(state.startup_variant, StartupVariant::QuinnOnly)
    {
      let custom_shutdown = move |handle: ServerHandle, http_handle: ServerHandle| async move {
        if reload_rx.recv().await.is_ok() {
          handle.stop_graceful(Duration::from_secs(10));
          http_handle.stop_graceful(Duration::from_secs(10));
        }
      };

      let (server, handle) = start_with_service(state.clone(), &setup, lbrp_service).await.unwrap();
      let (http_server, http_handle) = start_force_https_redirect(80, 443).await.unwrap();

      let h1 = handle.clone();
      let h2 = handle.clone();
      let http_h1 = http_handle.clone();
      let http_h2 = http_handle.clone();
      let custom_handle = tokio::spawn(async move { custom_shutdown(h1, http_h1).await });
      let default_handle = tokio::spawn(async move { default_shutdown_signal(h2, Some(http_h2)).await });

      tracing::info!("Server is booted.");

      select! {
        _ = server => tracing::info!("Server is shutdowned."),
        _ = http_server => tracing::info!("Server is shutdowned."),
        _ = custom_handle => tracing::info!("Server is going to reload..."),
        _ = default_handle => std::process::exit(0),
        res = watcher_handle => {
          tracing::info!("Watcher handle is stopped with result `{:?}`! Exit...", res);
          return Ok(())
        },
      }
    } else {
      let custom_shutdown = move |handle: ServerHandle| async move {
        if reload_rx.recv().await.is_ok() {
          handle.stop_graceful(Duration::from_secs(10));
        }
      };

      let (server, handle) = start_with_service(state.clone(), &setup, lbrp_service).await.unwrap();

      let h1 = handle.clone();
      let h2 = handle.clone();
      let custom_handle = tokio::spawn(async move { custom_shutdown(h1).await });
      let default_handle = tokio::spawn(async move { default_shutdown_signal(h2, None).await });

      tracing::info!("Server is booted.");

      select! {
        _ = server => tracing::info!("Server is shutdowned."),
        _ = custom_handle => tracing::info!("Server is going to reload..."),
        _ = default_handle => std::process::exit(0),
        res = watcher_handle => {
          tracing::info!("Watcher handle is stopped with result `{:?}`! Exit...", res);
          return Ok(())
        },
      }
    }
  }
}

async fn default_shutdown_signal(handle: ServerHandle, http_handle: Option<ServerHandle>) {
  tokio::signal::ctrl_c().await.unwrap();
  tracing::info!("Shutdown with Ctrl+C requested.");
  handle.stop_graceful(None);
  if let Some(h) = http_handle {
    h.stop_graceful(None);
  }
  std::process::exit(0);
}
