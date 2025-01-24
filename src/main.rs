// #![deny(warnings, clippy::todo, clippy::unimplemented)]
#![feature(let_chains)]

mod client;
mod config;
#[cfg(feature = "err-handler")]
mod error_handling;

use cc_server_kit::cc_utils::prelude::*;
use cc_server_kit::prelude::*;
use cc_server_kit::salvo::server::ServerHandle;
use cc_server_kit::startup::{get_root_router_autoinject, start_with_service};
use std::time::Duration;
use tokio::select;
use tokio::sync::broadcast;

use crate::client::ModifiedReqwestClient;
use crate::config::{config_watcher, LbrpConfig, Service};
#[cfg(feature = "err-handler")]
use error_handling::{error_files_handler, error_handler, error_index_handler, ERR_HANDLER};

#[derive(Default, Clone)]
struct Setup {
  generic_values: GenericValues,
}

impl GenericSetup for Setup {
  fn generic_values(&self) -> &GenericValues {
    &self.generic_values
  }
  fn set_generic_values(&mut self, generic_values: GenericValues) {
    self.generic_values = generic_values;
  }
}

fn get_router_from_config(config: &LbrpConfig, children: &mut Vec<std::process::Child>) -> Router {
  for child in children.iter_mut() {
    child.kill().unwrap();
  }
  children.clear();

  let mut router = Router::new();

  #[cfg(feature = "err-handler")]
  {
    router = router
      .push(Router::new().path("/400").get(error_index_handler))
      .push(Router::new().path("/401").get(error_index_handler))
      .push(Router::new().path("/403").get(error_index_handler))
      .push(Router::new().path("/404").get(error_index_handler))
      .push(Router::new().path("/405").get(error_index_handler))
      .push(Router::new().path("/423").get(error_index_handler))
      .push(Router::new().path("/500").get(error_index_handler));
  }

  for service in &config.services {
    if let Service::CommonService(service) = service {
      if service.should_startup() {
        children.push(service.startup().unwrap());
      }

      router = router.push(
        Router::new()
          .host(service.from.clone())
          .path("{**rest}")
          .goal(ModifiedReqwestClient::new_client(service.to.clone())),
      )
    }

    #[cfg(feature = "err-handler")]
    if let Service::ErrorHandler(err_handler) = service {
      let mut eh_router = Router::new();
      for file in &err_handler.static_files {
        let path = err_handler.dist_dir.join(file);
        eh_router = eh_router.push(
          Router::new()
            .path(format!("/{}", path.to_str().unwrap()))
            .get(error_files_handler),
        );
      }
      router = router.push(eh_router);
    }
  }

  router = router.push(Router::new().path("{**rest_path}").get(error_handler));
  router
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> MResult<()> {
  let (reload_tx, _) = broadcast::channel::<()>(16);

  let setup = load_generic_config::<Setup>("lbrp-service").await.unwrap();
  let state = load_generic_state(&setup).await.unwrap();
  let mut children = vec![];

  loop {
    let watcher_tx = reload_tx.clone();
    let watcher_handle = tokio::spawn(async move {
      if let Err(e) = config_watcher("lbrp-config.json", watcher_tx).await {
        eprintln!("Config watcher error: {:?}", e);
      }
    });

    let mut reload_rx = reload_tx.subscribe();
    let custom_shutdown = move |handle: ServerHandle| async move {
      if reload_rx.recv().await.is_ok() {
        handle.stop_graceful(Duration::from_secs(10));
      }
    };

    let file = std::fs::File::open("lbrp-config.json").map_err(|_| "Can't open `lbrp-config.json`!")?;
    let reader = std::io::BufReader::new(file);
    let config = match serde_json::from_reader::<_, LbrpConfig>(reader) {
      Ok(config) => {
        #[cfg(feature = "err-handler")]
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

    let lbrp_router =
      get_root_router_autoinject(&state, setup.clone()).push(get_router_from_config(&config, &mut children));

    tracing::debug!("\n{:?}", lbrp_router);

    #[allow(unused_mut)]
    let mut lbrp_service = salvo::Service::new(lbrp_router);

    #[cfg(feature = "err-handler")]
    if let Some(err_handler) = config.services.iter().find(|s| matches!(s, Service::ErrorHandler(_)))
      && let Service::ErrorHandler(err_handler) = err_handler
    {
      let mut guard = ERR_HANDLER.as_ref().lock().await;
      *guard = Some(err_handler.clone());
      lbrp_service = lbrp_service.catcher(salvo::catcher::Catcher::default().hoop(error_handler));
    }

    let (server, handle) = start_with_service(state.clone(), &setup, lbrp_service).await.unwrap();

    let h1 = handle.clone();
    let h2 = handle.clone();
    let custom_handle = tokio::spawn(async move { custom_shutdown(h1).await });
    let default_handle = tokio::spawn(async move { default_shutdown_signal(h2).await });

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

async fn default_shutdown_signal(handle: ServerHandle) {
  tokio::signal::ctrl_c().await.unwrap();
  tracing::info!("Shutdown with Ctrl+C requested.");
  handle.stop_graceful(None);
  std::process::exit(0);
}
