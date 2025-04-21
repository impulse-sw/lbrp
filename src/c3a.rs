use cc_server_kit::prelude::*;
use salvo::{
  http::cookie::{CookieBuilder, Expiration, SameSite},
  writing::Redirect,
  Request, Response,
};

#[handler]
pub(crate) async fn store(req: &mut Request, res: &mut Response) {
  let name = req.query::<String>("name");
  let cookie = req.query::<String>("cookie");
  let redirect = req.query::<String>("redirect");
  if name.is_none() && cookie.is_none() && redirect.is_none() {
    res.render(Redirect::found("/400"));
    return;
  }
  let name = name.unwrap();
  let cookie = cookie.unwrap();
  let redirect = redirect.unwrap();
  res.add_cookie(
    CookieBuilder::new(name, cookie)
      .secure(true)
      .http_only(true)
      .same_site(SameSite::Strict)
      .expires(Expiration::Session)
      .build(),
  );
  res.render(Redirect::found(&redirect));
}

#[handler]
pub(crate) async fn unstore(req: &mut Request, res: &mut Response) {
  let name = req.query::<String>("name");
  let redirect = req.query::<String>("redirect");
  if name.is_none() && redirect.is_none() {
    res.render(Redirect::found("/400"));
    return;
  }
  let name = name.unwrap();
  let redirect = redirect.unwrap();
  res.remove_cookie(&name);
  res.render(Redirect::found(&redirect));
}
