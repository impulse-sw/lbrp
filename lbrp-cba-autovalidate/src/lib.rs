#![deny(warnings, clippy::todo, clippy::unimplemented)]

use lbrp_cli_authorize::LbrpAuthorize;
use wasm_bindgen::prelude::*;

fn get_host() -> String {
  web_sys::window()
    .unwrap()
    .document()
    .unwrap()
    .location()
    .unwrap()
    .host()
    .unwrap()
    .to_string()
}

fn get_protocol() -> String {
  web_sys::window()
    .unwrap()
    .document()
    .unwrap()
    .location()
    .unwrap()
    .protocol()
    .unwrap()
    .to_string()
}

fn endpoint(api_uri: impl AsRef<str>) -> String {
  format!("{}//{}{}", get_protocol(), get_host(), api_uri.as_ref())
}

async fn sleep(delay: i32) {
  let mut cb = |resolve: js_sys::Function, _| {
    web_sys::window()
      .unwrap()
      .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, delay)
      .unwrap();
  };

  let p = js_sys::Promise::new(&mut cb);

  wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
}

#[wasm_bindgen]
pub fn cba_autovalidate() {
  wasm_bindgen_futures::spawn_local(async {
    loop {
      reqwest::Client::new()
        .get("/")
        .lbrp_authorize(endpoint("/--inner-lbrp-auth/revalidate"))
        .await
        .ok();
      sleep(3 * 60 * 1000).await;
    }
  });
}
