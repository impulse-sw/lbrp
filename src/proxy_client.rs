use cc_server_kit::cc_utils::errors::ServerError;
use cc_server_kit::reqwest;
use cc_server_kit::salvo;
use cc_server_kit::salvo::http::HeaderValue;
use cc_server_kit::tracing;
use futures_util::TryStreamExt;
use hyper::header::{CONNECTION, UPGRADE};
use hyper::upgrade::OnUpgrade;
use hyper::{HeaderMap, StatusCode};
use reqwest::Client as ReqwestCli;
use salvo::http::{ReqBody, ResBody};
use salvo::hyper;
use salvo::proxy::{Client as ProxyCli, Proxy, Upstreams};
use salvo::rt::tokio::TokioIo;
use tokio::io::copy_bidirectional;

use crate::config::CorsOpts;

#[derive(Clone, Debug)]
pub(crate) struct ModifiedReqwestClient {
  inner: ReqwestCli,
  cors: Option<Vec<String>>,
  cors_opts: CorsOpts,
}

#[allow(dead_code)]
impl ModifiedReqwestClient {
  /// Create a new `ModifiedReqwestClient` with the given [`reqwest::Client`].
  pub fn new(inner: ReqwestCli, cors: Option<Vec<String>>, cors_opts: CorsOpts) -> Self {
    Self { inner, cors, cors_opts }
  }

  pub fn new_client<U: Upstreams>(
    upstreams: U,
    cors: &Option<Vec<String>>,
    cors_opts: &CorsOpts,
  ) -> Proxy<U, ModifiedReqwestClient> {
    Proxy::new(
      upstreams,
      ModifiedReqwestClient::new(ReqwestCli::default(), cors.to_owned(), cors_opts.clone()),
    )
  }

  #[allow(clippy::wrong_self_convention)]
  pub fn as_client<U: Upstreams>(self, upstreams: U) -> Proxy<U, ModifiedReqwestClient> {
    Proxy::new(upstreams, self)
  }
}

type HyperRequest = hyper::Request<ReqBody>;
type HyperResponse = hyper::Response<ResBody>;

fn get_upgrade_type(headers: &HeaderMap) -> Option<&str> {
  if headers
    .get(&CONNECTION)
    .map(|value| {
      value
        .to_str()
        .unwrap_or_default()
        .split(',')
        .any(|e| e.trim() == UPGRADE)
    })
    .unwrap_or(false)
  {
    if let Some(upgrade_value) = headers.get(&UPGRADE) {
      tracing::debug!("Found upgrade header with value: {:?}", upgrade_value.to_str());
      return upgrade_value.to_str().ok();
    }
  }

  None
}

impl ProxyCli for ModifiedReqwestClient {
  type Error = ServerError;

  #[tracing::instrument(skip_all, fields(http.uri = proxied_request.uri().path(), http.method = proxied_request.method().as_str()))]
  async fn execute(
    &self,
    mut proxied_request: HyperRequest,
    request_upgraded: Option<OnUpgrade>,
  ) -> Result<HyperResponse, Self::Error> {
    tracing::info!(
      r#"Redirect to "{}:{}""#,
      proxied_request
        .uri()
        .host()
        .map(|v| v.to_string())
        .unwrap_or("undefined".to_string()),
      proxied_request
        .uri()
        .port()
        .map(|v| v.to_string())
        .unwrap_or("undefined".to_string()),
    );

    let origin = proxied_request.headers().get(hyper::header::ORIGIN).cloned();

    // CORS logic
    if let Some(cors) = &self.cors
      && proxied_request.method() == hyper::Method::OPTIONS
      && let Some(origin) = &origin
      && let Ok(origin) = origin.to_str()
      && cors.iter().any(|v| v.as_str().eq(origin))
    {
      tracing::trace!("Allowing `OPTIONS` request");
      let mut resp = HyperResponse::new(ResBody::None);
      resp.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(&self.cors_opts.allowed_methods).unwrap(),
      );
      resp.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_str(&self.cors_opts.allowed_headers).unwrap(),
      );
      resp.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_str(&self.cors_opts.allowed_client_headers).unwrap(),
      );
      resp
        .headers_mut()
        .insert(hyper::header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
      resp.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_str(origin).unwrap(),
      );
      resp.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
      );
      resp
        .headers_mut()
        .insert(hyper::header::VARY, HeaderValue::from_static("Cookie, Origin"));
      *resp.status_mut() = StatusCode::NO_CONTENT;
      tracing::trace!("{:?}", resp.headers());
      return Ok(resp);
    }

    if let Some(cors) = &self.cors
      && let Some(host) = &proxied_request.headers().get(hyper::header::HOST).cloned()
      && let Ok(host) = host.to_str()
      && let Some(origin) = &origin
      && let Ok(origin) = origin.to_str()
      && cors.iter().any(|v| v.as_str().eq(origin))
      && host.ne(origin)
    {
      tracing::trace!("Change `ORIGIN` to `HOST`");
      proxied_request
        .headers_mut()
        .insert(hyper::header::ORIGIN, HeaderValue::from_str(host).unwrap());
      tracing::trace!("{:?}", proxied_request.headers());
    }

    let request_upgrade_type = get_upgrade_type(proxied_request.headers()).map(|s| s.to_owned());

    let proxied_request =
      proxied_request.map(|s| reqwest::Body::wrap_stream(s.map_ok(|s| s.into_data().unwrap_or_default())));
    let response = self
      .inner
      .execute(proxied_request.try_into().map_err(|e| {
        ServerError::from_private(e)
          .with_public("Can't convert proxied request!")
          .with_500()
      })?)
      .await
      .map_err(|e| {
        ServerError::from_private(e)
          .with_public("Can't execute request!")
          .with_404()
      })?;

    let res_headers = response.headers().clone();
    let hyper_response = hyper::Response::builder()
      .status(response.status())
      .version(response.version());

    let mut hyper_response = if response.status() == StatusCode::SWITCHING_PROTOCOLS {
      let response_upgrade_type = get_upgrade_type(response.headers());

      if request_upgrade_type == response_upgrade_type.map(|s| s.to_lowercase()) {
        let mut response_upgraded = response.upgrade().await.map_err(|e| {
          ServerError::from_private(e)
            .with_public("Can't upgrade response!")
            .with_500()
        })?;
        if let Some(request_upgraded) = request_upgraded {
          tokio::spawn(async move {
            match request_upgraded.await {
              Ok(request_upgraded) => {
                let mut request_upgraded = TokioIo::new(request_upgraded);
                if let Err(e) = copy_bidirectional(&mut response_upgraded, &mut request_upgraded).await {
                  tracing::error!(error = ?e, "copying between upgraded connections failed");
                }
              }
              Err(e) => tracing::error!(error = ?e, "upgrade request failed"),
            }
          });
        } else {
          ServerError::from_private_str("request does not have an upgrade extension")
            .with_500()
            .bail()?;
        }
      } else {
        ServerError::from_private_str("upgrade type mismatch")
          .with_500()
          .bail()?;
      }
      hyper_response.body(ResBody::None).map_err(|e| {
        ServerError::from_private(e)
          .with_public("Can't set document body!")
          .with_500()
      })?
    } else {
      hyper_response
        .body(ResBody::stream(response.bytes_stream()))
        .map_err(|e| {
          ServerError::from_private(e)
            .with_public("Can't set document body!")
            .with_500()
        })?
    };
    *hyper_response.headers_mut() = res_headers;

    if let Some(cors) = &self.cors
      && let Some(origin) = &origin
      && let Ok(origin) = origin.to_str()
      && cors.iter().any(|v| v.as_str().eq(origin))
    {
      tracing::trace!("Allowing CORS on actual request");
      hyper_response.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(&self.cors_opts.allowed_methods).unwrap(),
      );
      hyper_response.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_str(&self.cors_opts.allowed_headers).unwrap(),
      );
      hyper_response.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_str(&self.cors_opts.allowed_client_headers).unwrap(),
      );
      hyper_response
        .headers_mut()
        .insert(hyper::header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
      hyper_response.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_str(origin).unwrap(),
      );
      hyper_response.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
      );
      hyper_response
        .headers_mut()
        .insert(hyper::header::VARY, HeaderValue::from_static("Cookie, Origin"));
      tracing::trace!("{:?}", hyper_response.headers());
    }

    Ok(hyper_response)
  }
}
