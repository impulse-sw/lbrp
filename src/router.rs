use cc_server_kit::prelude::*;
use salvo::Handler;

use crate::config::{LbrpConfig, Service};
use crate::error_handling::{error_files_handler, error_index_handler, proxied_error_handler};
use crate::proxy_client::ModifiedReqwestClient;
use crate::r#static::StaticRoute;

pub fn get_router_from_config(config: &LbrpConfig, children: &mut Vec<std::process::Child>) -> Router {
  for child in children.iter_mut() {
    child.kill().unwrap();
  }
  children.clear();

  let mut router = Router::new();

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
      router = router.push(Router::new().path(format!("/{}", file)).get(error_files_handler));
    }
  }

  for service in &config.services {
    if let Service::CommonService(service) = service {
      if service.should_startup() {
        children.push(service.startup().unwrap());
      }

      let mut service_router = Router::new().host(service.from.clone());

      if let Some(Service::CommonStatic(r#static)) =
        &config.services.iter().find(|v| matches!(v, Service::CommonStatic(_)))
      {
        for (route, path) in &r#static.static_routes {
          service_router = service_router.push(Router::with_path(route).get(StaticRoute::new(path)));
        }
      }

      #[cfg(feature = "c3a")]
      if let Some(tags) = &service.require_subdomain_auth {
        service_router = service_router
          .hoop(crate::c3a::MaybeC3ARedirect::new(tags.clone()))
          .push(crate::c3a::auth_router());
      }

      service_router = service_router.push({
        if config.services.iter().any(|s| matches!(s, Service::ErrorHandler(_)))
          && !service.skip_err_handling.is_some_and(|v| v)
        {
          Router::with_path("{**rest}").goal(
            ModifiedReqwestClient::new_client(service.to.clone(), &service.cors_domains, &config.cors_opts)
              .hoop(proxied_error_handler),
          )
        } else {
          Router::with_path("{**rest}").goal(ModifiedReqwestClient::new_client(
            service.to.clone(),
            &service.cors_domains,
            &config.cors_opts,
          ))
        }
      });
      router = router.push(service_router);
    }
  }

  router
}
