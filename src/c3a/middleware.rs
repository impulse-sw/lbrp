use c3a_server_sdk::c3a_common;
use cc_server_kit::prelude::*;
use cc_server_kit::{cc_utils::responses::ExplicitServerWrite, salvo::Writer};
use cc_utils::prelude::*;
use futures_util::StreamExt;

use crate::c3a::extract_authcli;

use super::auth_client::LbrpAuthMethods;

const AUTOUPDATER_INJECT_LINKS: &str = r#"<link rel="modulepreload" href="/--inner-lbrp-auth/lbrp_cba_autovalidate.js" crossorigin="anonymous"><link rel="preload" href="/--inner-lbrp-auth/lbrp_cba_autovalidate_bg.wasm" crossorigin="anonymous" as="fetch" type="application/wasm"></head"#;

const AUTOUPDATER_INJECT_SCRIPT: &str = r#"<script type="module">import init, * as autoupdBindings from '/--inner-lbrp-auth/lbrp_cba_autovalidate.js'; const wasm = await init({ module_or_path: '/--inner-lbrp-auth/lbrp_cba_autovalidate_bg.wasm' }); window.autoupdBindings = autoupdBindings; autoupdBindings.cba_autovalidate();</script></body"#;

pub(crate) struct MaybeC3ARedirect {
  pub(crate) tags: Vec<c3a_common::AppTag>,
}

impl MaybeC3ARedirect {
  pub(crate) fn new(tags: Vec<c3a_common::AppTag>) -> Self {
    Self { tags }
  }

  pub(crate) async fn inject_autoupdater_on_html(res: &mut Response) {
    if res.status_code.is_none_or(|s| s == salvo::http::StatusCode::OK)
      && res
        .content_type()
        .is_some_and(|ct| ct.subtype() == salvo::http::mime::HTML)
    {
      let body = res.take_body();
      tracing::debug!("Got body: {:?}", body);
      if let salvo::http::ResBody::Once(bytes) = &body
        && let Ok(html) = String::from_utf8(bytes.to_vec())
      {
        let site = html
          .replace("</head", AUTOUPDATER_INJECT_LINKS)
          .replace("</body", AUTOUPDATER_INJECT_SCRIPT);
        html!(site).unwrap().explicit_write(res).await;
        tracing::debug!("Rendered changed html");
      } else if let salvo::http::ResBody::Stream(stream) = body {
        tracing::debug!("Found stream; content length = {:?}", res.headers.get("Content-Length"));
        let mut stream = stream.into_inner();
        let mut collected_bytes = Vec::new();
        while let Some(frame) = stream.next().await {
          tracing::debug!("Got new frame");
          match frame {
            Ok(bytes_frame) => {
              tracing::debug!(
                "Frame is a bytes array with len = {}",
                bytes_frame.data_ref().unwrap().len()
              );
              collected_bytes.extend_from_slice(bytes_frame.data_ref().unwrap());
            }
            Err(_) => {
              ServerError::from_private_str("Can't collect frames! Got an error")
                .with_500()
                .explicit_write(res)
                .await;
              return;
            }
          }
        }
        match String::from_utf8(collected_bytes) {
          Err(e) => {
            ServerError::from_private(e)
              .with_private_str("Can't convert bytes into string!")
              .with_500()
              .explicit_write(res)
              .await;
            return;
          }
          Ok(html) => {
            let site = html
              .replace("</head", AUTOUPDATER_INJECT_LINKS)
              .replace("</body", AUTOUPDATER_INJECT_SCRIPT);
            res.headers_mut().remove("Content-Length");
            html!(site).unwrap().explicit_write(res).await;
            tracing::debug!("Rendered changed html");
          }
        }
      } else {
        res.body(body);
      }
    }
  }
}

#[cc_server_kit::salvo::async_trait]
impl cc_server_kit::salvo::Handler for MaybeC3ARedirect {
  #[instrument(skip_all, fields(http.uri = req.uri().path(), http.method = req.method().as_str()))]
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {
    if let Ok(auth_cli) = extract_authcli(depot) {
      if let Ok(resp) = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::check_signed_in(auth_cli, req, res).await
        && resp.authorized
      {
        tracing::debug!("Authorized, thus we calling `ctrl.call_next`");
        ctrl.call_next(req, depot, res).await;
        Self::inject_autoupdater_on_html(res).await;
      } else if [
        "/--inner-lbrp-auth/sign-up-step1",
        "/--inner-lbrp-auth/sign-up-step2",
        "/--inner-lbrp-auth/sign-in-step1",
        "/--inner-lbrp-auth/sign-in-step2",
        "/--inner-lbrp-auth/checkup",
        "/--inner-lbrp-auth/revalidate",
        "/--inner-lbrp-auth/lbrp-auth-frontend.js",
        "/--inner-lbrp-auth/lbrp-auth-frontend_bg.wasm",
        "/--inner-lbrp-auth/lbrp_cba_autovalidate.js",
        "/--inner-lbrp-auth/lbrp_cba_autovalidate_bg.wasm",
        "/--inner-lbrp-auth/tailwind.css",
      ]
      .contains(&req.uri().path())
      {
        ctrl.call_next(req, depot, res).await;
      } else if let Ok(site) = tokio::fs::read_to_string("lbrp-auth-frontend/dist/--inner-lbrp-auth/index.html").await {
        tracing::debug!("Unauthorized, thus we returning `html`");
        res.status_code(salvo::http::StatusCode::OK);
        res.render(salvo::writing::Text::Html(site));
      } else {
        ServerError::from_private_str("Can't read `lbrp-auth-frontend`!")
          .with_500()
          .write(req, depot, res)
          .await;
      }
      ctrl.skip_rest();
    } else {
      ServerError::from_private_str("Can't get `auth_cli`!")
        .with_500()
        .write(req, depot, res)
        .await;
    }
  }
}
