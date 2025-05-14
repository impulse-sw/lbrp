use c3a_client_sdk::Authorize;

fn get_host() -> String {
  web_sys::window()
    .ok_or::<String>("Can't get browser's window parameters.".into())
    .unwrap()
    .document()
    .ok_or::<String>("Can't get window's document.".into())
    .unwrap()
    .location()
    .ok_or::<String>("Can't get document's location.".into())
    .unwrap()
    .host()
    .map_err(|e| format!("Can't get host: {:?}", e))
    .unwrap()
    .to_string()
}

fn get_protocol() -> String {
  web_sys::window()
    .ok_or::<String>("Can't get browser's window parameters.".into())
    .unwrap()
    .document()
    .ok_or::<String>("Can't get window's document.".into())
    .unwrap()
    .location()
    .ok_or::<String>("Can't get document's location.".into())
    .unwrap()
    .protocol()
    .map_err(|e| format!("Can't get protocol: {:?}", e))
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

fn main() {
  wasm_bindgen_futures::spawn_local(async {
    loop {
      reqwest::Client::new()
        .get("/")
        .authorize(endpoint("/--inner-lbrp-auth/revalidate"))
        .await
        .ok();
      sleep(3 * 60 * 1000).await;
    }
  });
}
