use cc_server_kit::tracing;
use cc_server_kit::reqwest;
use cc_server_kit::salvo;
use cc_server_kit::utils::errors::ErrorResponse;
use futures_util::TryStreamExt;
use hyper::{HeaderMap, StatusCode};
use hyper::header::{CONNECTION, UPGRADE};
use hyper::upgrade::OnUpgrade;
use reqwest::Client as ReqwestCli;
use salvo::http::{ReqBody, ResBody};
use salvo::hyper;
use salvo::proxy::{Client as ProxyCli, Proxy, Upstreams};
use salvo::rt::tokio::TokioIo;
use tokio::io::copy_bidirectional;

#[derive(Default, Clone, Debug)]
pub(crate) struct ModifiedReqwestClient {
  inner: ReqwestCli,
}

#[allow(dead_code)]
impl ModifiedReqwestClient {
  /// Create a new `ModifiedReqwestClient` with the given [`reqwest::Client`].
  pub fn new(inner: ReqwestCli) -> Self {
    Self { inner }
  }
  
  pub fn new_client<U: Upstreams>(upstreams: U) -> Proxy<U, ModifiedReqwestClient> {
    Proxy::new(upstreams, ModifiedReqwestClient::default())
  }
  
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
  type Error = ErrorResponse;
  
  #[tracing::instrument(skip_all, fields(http.uri = proxied_request.uri().path(), http.method = proxied_request.method().as_str()))]
  async fn execute(
    &self,
    proxied_request: HyperRequest,
    request_upgraded: Option<OnUpgrade>,
  ) -> Result<HyperResponse, Self::Error> {
    tracing::info!(
      r#"Redirect to "{}:{}""#,
      proxied_request.uri().host().map(|v| v.to_string()).unwrap_or("undefined".to_string()),
      proxied_request.uri().port().map(|v| v.to_string()).unwrap_or("undefined".to_string()),
    );
    
    let request_upgrade_type = get_upgrade_type(proxied_request.headers()).map(|s| s.to_owned());

    let proxied_request = proxied_request.map(|s| reqwest::Body::wrap_stream(s.map_ok(|s| s.into_data().unwrap_or_default())));
    let response = self
      .inner
      .execute(proxied_request.try_into().map_err(|e| ErrorResponse::from(e))?)
      .await
      .map_err(|e| ErrorResponse::from(e))?;

    let res_headers = response.headers().clone();
    let hyper_response = hyper::Response::builder()
      .status(response.status())
      .version(response.version());

    let mut hyper_response = if response.status() == StatusCode::SWITCHING_PROTOCOLS {
      let response_upgrade_type = get_upgrade_type(response.headers());

      if request_upgrade_type == response_upgrade_type.map(|s| s.to_lowercase()) {
        let mut response_upgraded = response
          .upgrade()
          .await
          .map_err(|e| ErrorResponse::from(e))?;
        if let Some(request_upgraded) = request_upgraded {
          tokio::spawn(async move {
            match request_upgraded.await {
              Ok(request_upgraded) => {
                let mut request_upgraded = TokioIo::new(request_upgraded);
                if let Err(e) = copy_bidirectional(&mut response_upgraded, &mut request_upgraded).await {
                  tracing::error!(error = ?e, "coping between upgraded connections failed");
                }
              }
              Err(e) => tracing::error!(error = ?e, "upgrade request failed"),
            }
          });
        } else { return Err(ErrorResponse::from("request does not have an upgrade extension")); }
      } else { return Err(ErrorResponse::from("upgrade type mismatch")); }
      hyper_response.body(ResBody::None).map_err(|e| ErrorResponse::from(e.to_string()))?
    } else {
      hyper_response
        .body(ResBody::stream(response.bytes_stream()))
        .map_err(|e| ErrorResponse::from(e.to_string()))?
    };
    *hyper_response.headers_mut() = res_headers;
    Ok(hyper_response)
  }
}
