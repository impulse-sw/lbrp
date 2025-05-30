use cc_server_kit::prelude::*;
use cc_server_kit::salvo::hyper;
use hyper::header::HeaderValue;

use crate::config::CorsOpts;

pub struct CorsHandler {
  pub domain_cors_origins: Vec<String>,
  pub cors_opts: CorsOpts,
}

impl CorsHandler {
  pub(crate) fn new(origins: Vec<String>, cors_opts: CorsOpts) -> Self {
    Self {
      domain_cors_origins: origins,
      cors_opts,
    }
  }
}

#[cc_server_kit::salvo::async_trait]
impl cc_server_kit::salvo::Handler for CorsHandler {
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {
    let origin = req.headers().get(hyper::header::ORIGIN).cloned();
    let cors_matched = if let Some(origin) = &origin
      && let Ok(origin) = origin.to_str()
      && self.domain_cors_origins.iter().any(|o| o.as_str().eq(origin))
    {
      true
    } else {
      false
    };

    if req.method() == hyper::Method::OPTIONS && cors_matched {
      tracing::trace!("Allowing `OPTIONS` request");
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
          HeaderValue::from_str(&self.cors_opts.allowed_methods).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
          HeaderValue::from_str(&self.cors_opts.allowed_headers).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
          HeaderValue::from_str(&self.cors_opts.allowed_client_headers).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_MAX_AGE,
          HeaderValue::from_static("86400"),
          true,
        )
        .unwrap();
      res
        .add_header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.unwrap(), true)
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
          HeaderValue::from_static("true"),
          true,
        )
        .unwrap();
      res
        .add_header(hyper::header::VARY, HeaderValue::from_static("Cookie, Origin"), true)
        .unwrap();
      res.status_code(StatusCode::NO_CONTENT);
      ctrl.skip_rest();
      return;
    }

    ctrl.call_next(req, depot, res).await;

    if cors_matched {
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
          HeaderValue::from_str(&self.cors_opts.allowed_methods).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
          HeaderValue::from_str(&self.cors_opts.allowed_headers).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
          HeaderValue::from_str(&self.cors_opts.allowed_client_headers).unwrap(),
          true,
        )
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_MAX_AGE,
          HeaderValue::from_static("86400"),
          true,
        )
        .unwrap();
      res
        .add_header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.unwrap(), true)
        .unwrap();
      res
        .add_header(
          hyper::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
          HeaderValue::from_static("true"),
          true,
        )
        .unwrap();
      res
        .add_header(hyper::header::VARY, HeaderValue::from_static("Cookie, Origin"), true)
        .unwrap();
      res.status_code(StatusCode::NO_CONTENT);
    }
  }
}
