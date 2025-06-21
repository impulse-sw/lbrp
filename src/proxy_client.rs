use cc_server_kit::prelude::*;
use cc_server_kit::tracing::Instrument;
use futures_util::TryStreamExt;
use hyper::header::{CONNECTION, UPGRADE};
use hyper::upgrade::OnUpgrade;
use hyper::{HeaderMap, StatusCode};
use reqwest::Client as ReqwestCli;
use salvo::http::{HeaderValue, ReqBody, ResBody};
use salvo::hyper;
use salvo::proxy::{Client as ProxyCli, Proxy, Upstreams};
use salvo::rt::tokio::TokioIo;
use tokio::io::copy_bidirectional;

#[derive(Clone, Debug)]
pub(crate) struct ModifiedReqwestClient {
  inner: ReqwestCli,
  domain: String,
}

pub(crate) struct ProxyProvider {
  pub header_name: String,
}

#[cc_server_kit::salvo::async_trait]
impl cc_server_kit::salvo::Handler for ProxyProvider {
  #[tracing::instrument(
    skip_all,
    name = "provide-ip-addr",
    level = "debug",
    fields(
      http.uri = req.uri().path(),
      http.method = req.method().as_str()
    )
  )]
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {
    let from_ip = req.remote_addr().to_string();
    let hname = self.header_name.clone();
    req.headers_mut().insert(
      Box::leak(hname.into_boxed_str()) as &str,
      HeaderValue::from_str(&from_ip).unwrap(),
    );
    ctrl.call_next(req, depot, res).await;
  }
}

#[allow(dead_code)]
impl ModifiedReqwestClient {
  /// Create a new `ModifiedReqwestClient` with the given [`reqwest::Client`].
  pub fn new(inner: ReqwestCli, server_domain: &str) -> Self {
    Self {
      inner,
      domain: server_domain.to_owned(),
    }
  }

  pub fn new_client<U: Upstreams>(upstreams: U, server_domain: &str) -> Proxy<U, ModifiedReqwestClient> {
    Proxy::new(
      upstreams,
      ModifiedReqwestClient::new(
        ReqwestCli::builder()
          .redirect(reqwest::redirect::Policy::none())
          .build()
          .unwrap(),
        server_domain,
      ),
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
    && let Some(upgrade_value) = headers.get(&UPGRADE)
  {
    tracing::debug!("Found upgrade header with value: {:?}", upgrade_value.to_str());
    return upgrade_value.to_str().ok();
  }

  None
}

impl ProxyCli for ModifiedReqwestClient {
  type Error = ServerError;

  #[tracing::instrument(
    skip_all,
    name = "proxy-request",
    fields(
      http.domain = self.domain,
      http.uri = proxied_request.uri().path(),
      http.method = proxied_request.method().as_str()
    )
  )]
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

    proxied_request
      .headers_mut()
      .insert("host", HeaderValue::from_str(&self.domain).unwrap());

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
      .instrument(tracing::debug_span!("reqwest::execute"))
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
        let mut response_upgraded = response
          .upgrade()
          .instrument(tracing::debug_span!("reqwest::response_upgrade"))
          .await
          .map_err(|e| {
            ServerError::from_private(e)
              .with_public("Can't upgrade response!")
              .with_500()
          })?;
        if let Some(request_upgraded) = request_upgraded {
          tokio::spawn(async move {
            match request_upgraded
              .instrument(tracing::debug_span!("reqwest::request_upgrade"))
              .await
            {
              Ok(request_upgraded) => {
                let mut request_upgraded = TokioIo::new(request_upgraded);
                if let Err(e) = copy_bidirectional(&mut response_upgraded, &mut request_upgraded)
                  .instrument(tracing::debug_span!("reqwest::bidirectional_copy"))
                  .await
                {
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

    Ok(hyper_response)
  }
}
