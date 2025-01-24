use cc_server_kit::prelude::*;
use cc_utils::prelude::*;
use salvo::{prelude::Redirect, Response};
use tracing::warn;

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
  {
    let path = handler.dist_dir.join(&filename);
    file_upload!(path.as_path().to_string_lossy().to_string(), filename)
  } else {
    Err(ErrorResponse::from("Not found!").with_404_pub().build())
  }
}

#[handler]
pub(crate) async fn error_handler(res: &mut Response) {
  let guard = ERR_HANDLER.as_ref().lock().await;
  if let Some(handler) = guard.as_ref()
    && let Ok(data) = tokio::fs::read_to_string(handler.dist_dir.join("index.html")).await
  {
    // res.status_code(StatusCode::NOT_FOUND);
    res.render(salvo::writing::Text::Html(data));
  } else {
    res.render(salvo::writing::Text::Plain("Not found!"));
  }

  // res.render(Redirect::found(&format!(
  //   "/{}",
  //   res.status_code.unwrap_or(StatusCode::NOT_FOUND).as_u16()
  // )))
}
