use cc_server_kit::prelude::*;
use cc_utils::prelude::*;
use salvo::{http::ResBody, FlowCtrl, Response};

use crate::config::ErrorHandler;

pub static ERR_HANDLER: std::sync::LazyLock<std::sync::Arc<tokio::sync::Mutex<Option<ErrorHandler>>>> =
  std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Mutex::new(None)));

#[handler]
pub(crate) async fn error_index_handler() -> MResult<File> {
  let guard = ERR_HANDLER.as_ref().lock().await;
  if let Some(handler) = guard.as_ref() {
    let path = handler.dist_dir.join("index.html");
    file_upload!(path.as_path().to_string_lossy().to_string(), "index.html".to_string())
  } else {
    Err(ErrorResponse::from("Not found!").with_404_pub().build())
  }
}

#[handler]
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
    file_upload!(path.as_path().to_string_lossy().to_string(), filename)
  } else {
    Err(ErrorResponse::from("Not found!").with_404_pub().build())
  }
}

#[handler]
pub(crate) async fn error_handler(res: &mut Response, ctrl: &mut FlowCtrl) {
  let guard = ERR_HANDLER.as_ref().lock().await;
  if res.status_code.is_none_or(|s| s != StatusCode::OK)
    && let Some(handler) = guard.as_ref()
    && let Ok(data) = tokio::fs::read_to_string(handler.dist_dir.join("index.html")).await
  {
    let body = res.body.take();
    let body = match body {
      ResBody::None => "None".to_string(),
      ResBody::Once(bytes) => String::from_utf8_lossy_owned(bytes.to_vec()),
      _ => "<some>".to_string(),
    };
    tracing::warn!(
      "Got an error from proxied request: status code {}, error body: `{}`",
      res.status_code.unwrap_or(StatusCode::NOT_FOUND),
      body
    );
    res.status_code(res.status_code.unwrap_or(StatusCode::NOT_FOUND));
    res.render(salvo::writing::Text::Html(data));
    ctrl.skip_rest();
  }
}

#[handler]
pub(crate) async fn proxied_error_handler(
  req: &mut Request,
  res: &mut Response,
  depot: &mut Depot,
  ctrl: &mut FlowCtrl,
) {
  ctrl.call_next(req, depot, res).await;

  let guard = ERR_HANDLER.as_ref().lock().await;
  if res.status_code.is_none_or(|s| s != StatusCode::OK)
    && let Some(handler) = guard.as_ref()
    && tokio::fs::try_exists(handler.dist_dir.join("index.html"))
      .await
      .is_ok_and(|exists| exists)
  {
    let body = res.take_body();
    let body = match body {
      ResBody::None => "None".to_string(),
      ResBody::Once(bytes) => String::from_utf8_lossy_owned(bytes.to_vec()),
      ResBody::Chunks(_) => "<chunk>".to_string(),
      ResBody::Hyper(_) => "<hyper>".to_string(),
      ResBody::Boxed(_) => "<boxed>".to_string(),
      ResBody::Stream(_) => "<stream>".to_string(),
      ResBody::Channel(_) => "<channel>".to_string(),
      ResBody::Error(_) => "<error>".to_string(),
      _ => "<idk>".to_string(),
    };
    tracing::warn!(
      "Got an error from proxied request: status code {:?}, error body: `{}`",
      res.status_code,
      body
    );
    res.headers_mut().clear();
    *res.body_mut() = ResBody::None;

    let status = res.status_code.unwrap_or(StatusCode::NOT_FOUND);
    res.status_code(status);
    res.headers_mut().insert(
      salvo::http::header::LOCATION,
      salvo::http::header::HeaderValue::from_str(&format!("/{}", status.as_u16())).unwrap(),
    );
  }
}
