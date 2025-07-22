use impulse_server_kit::prelude::*;
use impulse_server_kit::salvo::prelude::{Compression, CompressionLevel};

use crate::config::{LbrpConfig, Service};
use crate::cors_handling::CorsHandler;
use crate::error_handling::{ERR_HANDLER, error_files_handler, error_index_handler, proxied_error_handler};
use crate::proxy_client::{ModifiedReqwestClient, ProxyProvider};

pub fn excluded_from_err_handling(services: &[Service]) -> Vec<String> {
  services
    .iter()
    .filter_map(|s| {
      if let Service::CommonService(common) = s {
        Some(common)
      } else {
        None
      }
    })
    .filter_map(|s| {
      if s.skip_err_handling.is_some_and(|v| v) {
        Some(s.from.to_string())
      } else {
        None
      }
    })
    .collect::<Vec<_>>()
}

pub async fn get_router_from_config(config: &LbrpConfig, children: &mut Vec<std::process::Child>) -> Router {
  for child in children.iter_mut() {
    child.kill().unwrap();
  }
  children.clear();

  let mut router = Router::with_hoop(Compression::new().disable_all().enable_zstd(CompressionLevel::Fastest));

  if let Some(Service::ErrorHandler(err_handler)) =
    config.services.iter().find(|s| matches!(s, Service::ErrorHandler(_)))
  {
    router = router
      .push(Router::new().path("/400").get(error_index_handler))
      .push(Router::new().path("/401").get(error_index_handler))
      .push(Router::new().path("/403").get(error_index_handler))
      .push(Router::new().path("/404").get(error_index_handler))
      .push(Router::new().path("/405").get(error_index_handler))
      .push(Router::new().path("/423").get(error_index_handler))
      .push(Router::new().path("/500").get(error_index_handler))
      .push(Router::new().path("/oops").get(error_index_handler));

    for file in &err_handler.static_files {
      router = router.push(Router::new().path(format!("/{file}")).get(error_files_handler));
    }

    let mut err_handler = err_handler.clone();
    err_handler.static_files = err_handler
      .static_files
      .into_iter()
      .map(|fp| format!("/{fp}"))
      .collect::<_>();

    let mut guard = ERR_HANDLER.as_ref().lock().await;
    *guard = Some(err_handler);
  }

  for service in &config.services {
    if let Service::CommonService(service) = service {
      if service.should_startup() {
        children.push(service.startup().unwrap());
      }

      let mut service_router = Router::new().host(service.from.clone());

      if let Some(header_name) = service.provide_ip_as_header.as_deref() {
        service_router = service_router.hoop(ProxyProvider {
          header_name: header_name.to_string(),
        });
      }

      #[cfg(feature = "authnz")]
      if let Some(tags) = &service.require_subdomain_auth {
        service_router = service_router
          .hoop(crate::authnz::MaybeC3ARedirect::new(tags.clone()))
          .push(crate::authnz::auth_router());
      }

      let mut rest_router = if let Some(Service::CommonStatic(r#static)) =
        &config.services.iter().find(|v| matches!(v, Service::CommonStatic(_)))
      {
        Router::with_path("{**rest_path}")
          .hoop(
            impulse_static_server::StaticRouter::new(&r#static.path)
              .unwrap()
              .with_routes_list(r#static.static_routes.clone()),
          )
          .goal(ModifiedReqwestClient::new_client(service.to.clone(), &service.from))
      } else {
        Router::with_path("{**rest_path}").goal(ModifiedReqwestClient::new_client(service.to.clone(), &service.from))
      };

      if config.services.iter().any(|s| matches!(s, Service::ErrorHandler(_)))
        && !service.skip_err_handling.is_some_and(|v| v)
      {
        rest_router = rest_router.hoop(proxied_error_handler);
      }

      if let Some(origins) = service.cors_domains.as_ref().cloned() {
        rest_router = rest_router.hoop(CorsHandler::new(origins, config.cors_opts.clone()));
      }

      service_router = service_router.push(rest_router);
      router = router.push(service_router);
    }
  }

  router
}
