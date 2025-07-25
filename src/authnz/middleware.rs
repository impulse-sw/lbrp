use authnz_server_sdk::authnz_common;
use futures_util::StreamExt;
use impulse_server_kit::prelude::*;
use impulse_server_kit::{impulse_utils::responses::ExplicitServerWrite, salvo::Writer};

use crate::authnz::extract_authcli;

const AUTOUPDATER_INJECT_LINKS: &str = r#"<link rel="modulepreload" href="/--inner-lbrp-auth/lbrp_cba_autovalidate.js" crossorigin="anonymous"><link rel="preload" href="/--inner-lbrp-auth/lbrp_cba_autovalidate_bg.wasm" crossorigin="anonymous" as="fetch" type="application/wasm"></head"#;

const AUTOUPDATER_INJECT_SCRIPT: &str = r#"<script type="module">import init, * as autoupdBindings from '/--inner-lbrp-auth/lbrp_cba_autovalidate.js'; const wasm = await init({ module_or_path: '/--inner-lbrp-auth/lbrp_cba_autovalidate_bg.wasm' }); window.autoupdBindings = autoupdBindings; autoupdBindings.cba_autovalidate();</script></body"#;

pub(crate) struct MaybeC3ARedirect {
  pub(crate) tags: Vec<authnz_common::AccessTag>,
}

impl MaybeC3ARedirect {
  pub(crate) fn new(tags: Vec<authnz_common::AccessTag>) -> Self {
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
        tracing::debug!("RESPONSE HEADERS: {:?}", res.headers());
        let mut stream = stream.into_inner();
        let mut collected_bytes = if let Some(cl) = res.headers.get("Content-Length")
          && let Ok(sz) = cl.to_str()
          && let Ok(sz) = sz.parse::<usize>()
        {
          Vec::with_capacity(sz)
        } else {
          Vec::new()
        };
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
        tracing::debug!("Collected bytes: {:?}", String::from_utf8_lossy(&collected_bytes));
        match String::from_utf8(collected_bytes) {
          Err(e) => {
            ServerError::from_private(e)
              .with_private_str("Can't convert bytes into string!")
              .with_500()
              .explicit_write(res)
              .await;
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

#[impulse_server_kit::salvo::async_trait]
impl impulse_server_kit::salvo::Handler for MaybeC3ARedirect {
  async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut salvo::FlowCtrl) {
    if let Ok(auth_cli) = extract_authcli(depot) {
      if let Ok(resp) = auth_cli.check_signed_in(req, res).await
        && resp.authorized
      {
        if let Ok(tags) = auth_cli
          .get_user_tags(authnz_common::Id::Nickname {
            nickname: "archibald-host".to_string(),
          })
          .await
        {
          tracing::debug!("Signed in, tags: {:?}", tags);
          if let Ok(resp) = auth_cli.check_authorized_to(req, res, &self.tags).await
            && resp.authorized
          {
            tracing::debug!("AUTHORIZED FOR TAGS: {:?}", self.tags);
            let encodings = req.headers_mut().remove("Accept-Encoding");
            ctrl.call_next(req, depot, res).await;
            if let Some(encodings) = encodings {
              req.headers_mut().insert("Accept-Encoding", encodings);
            }
            Self::inject_autoupdater_on_html(res).await;
          } else {
            tracing::debug!("UNAUTHORIZED FOR TAGS: {:?}", self.tags);
            ServerError::from_private_str("Unauthorized for requested tags.")
              .with_403()
              .write(req, depot, res)
              .await;
          }
        }
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
        "/favicon.ico",
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
