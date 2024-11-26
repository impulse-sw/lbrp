#![feature(async_closure)]

mod config;
mod client;

use cc_server_kit::prelude::*;
use cc_server_kit::salvo::server::ServerHandle;
use cc_server_kit::startup::start_with_custom_shutdown;
use std::time::Duration;
use tokio::select;
use tokio::sync::broadcast;

use crate::config::{LbrpConfig, config_watcher};
use crate::client::ModifiedReqwestClient;

#[derive(Default, Clone)]
struct Setup {
  generic_values: GenericValues,
}

impl GenericSetup for Setup {
  fn generic_values(&self) -> &GenericValues { &self.generic_values }
  fn set_generic_values(&mut self, generic_values: GenericValues) { self.generic_values = generic_values; }
}

#[handler]
#[tracing::instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
async fn cat_request(req: &mut Request) -> MResult<()> {
  let authority = req.uri().authority();
  let headers = req.headers();
  let body = req.body();
  tracing::info!("Got request:\n\tauthority = {:?},\n\theaders = {:?},\n\tbody = {:?}", authority, headers, body);
  
  Ok(())
}

fn get_router_from_config(config: &LbrpConfig) -> Router {
  let mut router = Router::new();
  
  for service in &config.services {
    router = router.push(
      Router::new()
        .host(service.from.clone())
        .path("<**rest>")
        .goal(ModifiedReqwestClient::new_client(service.to.clone()))
    )
  }
  
  router = router.get(cat_request).push(Router::with_path("<**rest>").get(cat_request));
  
  router
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> MResult<()> {
  let (reload_tx, _) = broadcast::channel::<()>(16);
  
  let setup = load_generic_config::<Setup>("lbrp-service").await.unwrap();
  let state = load_generic_state(&setup).await.unwrap();
  
  loop {
    let watcher_tx = reload_tx.clone();
    let watcher_handle = tokio::spawn(async move {
      if let Err(e) = config_watcher("lbrp-config.json", watcher_tx).await {
        eprintln!("Config watcher error: {:?}", e);
      }
    });
    
    let mut reload_rx = reload_tx.subscribe();
    let custom_shutdown = async move |handle: ServerHandle| {
      if let Ok(_) = reload_rx.recv().await {
        handle.stop_graceful(Duration::from_secs(10));
      }
    };
    
    let file = std::fs::File::open("lbrp-config.json").map_err(|_| "Can't open `lbrp-config.json`!")?;
    let reader = std::io::BufReader::new(file);
    let config: LbrpConfig = match serde_json::from_reader(reader) {
      Ok(config) => config,
      Err(e) => {
        tracing::info!("Can't get the config due to: {}! Writing an example...", e);
        let file = std::fs::File::create("lbrp-config.json").unwrap();
        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &LbrpConfig::default()).unwrap();
        LbrpConfig::default()
      },
    };
    
    let lbrp_router = get_root_router(&state, setup.clone()).push(get_router_from_config(&config));
    
    tracing::debug!("\n{:?}", lbrp_router);
    
    let (server, _handle) = start_with_custom_shutdown(
      state.clone(),
      &setup,
      lbrp_router,
      Some(custom_shutdown),
    ).await.unwrap();
    
    tracing::info!("Server is booted.");
    
    select! {
      _ = server => {
        tracing::info!("Server is shutdowned.");
      },
      res = watcher_handle => {
        tracing::info!("Watcher handle is stopped with result `{:?}`! Exit...", res);
        return Ok(())
      },
    }
  }
}
