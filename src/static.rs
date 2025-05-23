use cc_server_kit::cc_utils::prelude::*;
use cc_server_kit::prelude::*;
use std::path::{Path, PathBuf};

pub(crate) struct StaticRoute {
  path: PathBuf,
}

impl StaticRoute {
  pub(crate) fn new(path: impl AsRef<Path>) -> Self {
    Self {
      path: path.as_ref().to_owned(),
    }
  }
}

#[handler]
impl StaticRoute {
  #[tracing::instrument(skip_all, name = "static-route", fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
  async fn handle(&self, req: &mut Request) -> MResult<File> {
    let filename = self.path.file_name().unwrap().to_string_lossy().to_string();
    file_upload!(self.path.clone(), filename)
  }
}
