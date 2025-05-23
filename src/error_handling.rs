use cc_server_kit::prelude::*;
use salvo::{FlowCtrl, Response, http::ResBody};

use crate::config::ErrorHandler;

pub static ERR_HANDLER: std::sync::LazyLock<std::sync::Arc<tokio::sync::Mutex<Option<ErrorHandler>>>> =
  std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Mutex::new(None)));

#[handler]
#[tracing::instrument(
  skip_all,
  name = "error-delivery",
  level = "debug",
  fields(
    http.uri = req.uri().path(),
    http.method = req.method().as_str()
  )
)]
pub(crate) async fn error_index_handler(req: &mut Request, res: &mut Response) {
  let guard = ERR_HANDLER.as_ref().lock().await;
  if let Some(handler) = guard.as_ref()
    && let Ok(data) = tokio::fs::read_to_string(handler.dist_dir.join("index.html")).await
  {
    tracing::warn!(
      "From proxied request: remote addr: {:?}, requested URL: `{}`, status code {:?}",
      req.remote_addr(),
      req.uri(),
      res.status_code,
    );
    res.status_code(res.status_code.unwrap_or(StatusCode::NOT_FOUND));
    res.render(salvo::writing::Text::Html(data));
  }
}

#[handler]
#[tracing::instrument(
  skip_all,
  name = "error-delivery",
  level = "debug",
  fields(
    http.uri = req.uri().path(),
    http.method = req.method().as_str()
  )
)]
pub(crate) async fn error_files_handler(req: &mut Request) -> MResult<File> {
  let guard = ERR_HANDLER.as_ref().lock().await;
  if let Some(handler) = guard.as_ref()
    && let Some(filename) = handler
      .static_files
      .iter()
      .map(|i| format!("/{}", i))
      .find(|el| el.as_str().eq(req.uri().path()))
      .map(|i| i.replace("/", ""))
  {
    let path = handler.dist_dir.join(&filename);
    tracing::debug!(
      "Path: {:?}, handler's dist dir: {:?}, filename: {}",
      path.as_path().to_string_lossy().to_string(),
      handler.dist_dir,
      filename
    );
    file_upload!(path, filename)
  } else {
    ServerError::from_public("Not found!").with_404().bail()
  }
}

pub(crate) struct ErrHandler {
  pub(crate) excluded: Vec<String>,
}

impl ErrHandler {
  pub(crate) fn new(excluded: Vec<String>) -> Self {
    Self { excluded }
  }
}

#[cc_server_kit::salvo::async_trait]
impl cc_server_kit::salvo::Handler for ErrHandler {
  #[tracing::instrument(
    skip_all,
    name = "error-delivery",
    level = "debug",
    fields(
      http.uri = req.uri().path(),
      http.method = req.method().as_str()
    )
  )]
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {
    let origin = req.headers().get(cc_server_kit::salvo::hyper::header::ORIGIN).cloned();
    ctrl.call_next(req, depot, res).await;

    let exclude_matched = if let Some(origin) = &origin
      && let Ok(origin) = origin.to_str()
      && self
        .excluded
        .iter()
        .any(|o| format!("https://{}", o).as_str().eq(origin))
    {
      true
    } else {
      false
    };

    if !exclude_matched {
      let guard = ERR_HANDLER.as_ref().lock().await;
      if res.status_code.is_none_or(|s| s.as_u16() >= 400u16)
        && let Some(handler) = guard.as_ref()
        && let Ok(data) = tokio::fs::read_to_string(handler.dist_dir.join("index.html")).await
      {
        tracing::warn!(
          "From proxied request: remote addr: {:?}, requested URL: `{}`, status code {:?}",
          req.remote_addr(),
          req.uri(),
          res.status_code,
        );
        res.status_code(res.status_code.unwrap_or(StatusCode::NOT_FOUND));
        res.render(salvo::writing::Text::Html(data));
        ctrl.skip_rest();
      }
    }
  }
}

#[handler]
#[tracing::instrument(
  skip_all,
  name = "error-delivery",
  level = "debug",
  fields(
    http.uri = req.uri().path(),
    http.method = req.method().as_str()
  )
)]
pub(crate) async fn proxied_error_handler(
  req: &mut Request,
  res: &mut Response,
  depot: &mut Depot,
  ctrl: &mut FlowCtrl,
) {
  ctrl.call_next(req, depot, res).await;

  let guard = ERR_HANDLER.as_ref().lock().await;
  if res.status_code.is_none_or(|s| s.as_u16() >= 400u16)
    && let Some(handler) = guard.as_ref()
    && tokio::fs::try_exists(handler.dist_dir.join("index.html"))
      .await
      .is_ok_and(|exists| exists)
  {
    tracing::warn!(
      "From proxied request: remote addr: {:?}, requested URL: `{}`, status code {:?}",
      req.remote_addr(),
      req.uri(),
      res.status_code,
    );

    res.headers_mut().remove(salvo::http::header::CONTENT_LENGTH);
    res.headers_mut().remove(salvo::http::header::CONTENT_TYPE);
    *res.body_mut() = ResBody::None;

    let status = res.status_code.unwrap_or(StatusCode::NOT_FOUND);
    res.status_code(status);
    res.headers_mut().insert(
      salvo::http::header::LOCATION,
      salvo::http::header::HeaderValue::from_str(&format!("/{}", status.as_u16())).unwrap(),
    );

    ctrl.skip_rest();
  }
}
