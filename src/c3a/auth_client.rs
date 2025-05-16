use c3a_common::TokenTriple;
use c3a_server_sdk::{
  C3AClient, C3AClientError,
  c3a_common::{self, AppTag},
};
use cc_server_kit::{prelude::*, salvo::http::cookie::CookieBuilder};

pub(crate) trait LbrpAuthMethods {
  fn _cookie<'a>(
    name: impl Into<std::borrow::Cow<'a, str>>,
    value: impl Into<std::borrow::Cow<'a, str>>,
  ) -> cc_server_kit::salvo::http::cookie::Cookie<'a>;
  fn _deploy_cookie<'a>(res: &'a mut Response, prefix: &'a str, value: &'a str);
  fn _collect_cookies(req: &Request, prefix: &str) -> String;
  fn _remove_cookies(res: &mut Response, prefix: &str);
  fn _try_collect_from_cookies(req: &Request) -> Option<TokenTriple>;
  fn _try_collect(req: &Request) -> Result<TokenTriple, C3AClientError>;
  fn deploy_triple_to_cookies(triple: &TokenTriple, res: &mut Response);
  async fn check_signed_in(&self, req: &mut Request, res: &mut Response) -> MResult<c3a_server_sdk::AuthorizeResponse>;
  async fn check_authorized_to(
    &self,
    req: &mut Request,
    res: &mut Response,
    tags: &[AppTag],
  ) -> MResult<c3a_server_sdk::AuthorizeResponse>;
  async fn update_client_token(&self, req: &mut Request, res: &mut Response) -> MResult<OK>;

  #[allow(dead_code)]
  async fn logout(&self, req: &mut Request, res: &mut Response) -> MResult<()>;
}

impl LbrpAuthMethods for C3AClient {
  fn _cookie<'a>(
    name: impl Into<std::borrow::Cow<'a, str>>,
    value: impl Into<std::borrow::Cow<'a, str>>,
  ) -> cc_server_kit::salvo::http::cookie::Cookie<'a> {
    CookieBuilder::new(name, value)
      .path("/")
      .secure(true)
      .http_only(true)
      .build()
  }

  fn _deploy_cookie<'a>(res: &'a mut Response, prefix: &'a str, value: &'a str) {
    res.add_cookie(<c3a_server_sdk::C3AClient as LbrpAuthMethods>::_cookie(
      prefix.to_string(),
      value.to_string(),
    ));
  }

  fn _collect_cookies(req: &Request, prefix: &str) -> String {
    if let Some(token) = req.cookie(prefix) {
      return token.value().to_string();
    }

    let mut i = 1u16;
    let mut parts = vec![];
    while let Some(part) = req.cookie(format!("{}-{}", prefix, i)) {
      parts.push(part.value().to_string());
      i += 1;
    }
    c3a_common::unite_token(parts)
  }

  fn _remove_cookies(res: &mut Response, prefix: &str) {
    let mut i = 1u16;
    while res.cookie(format!("{}-{}", prefix, i)).is_some() {
      res.cookies_mut().remove(format!("{}-{}", prefix, i));
      i += 1;
    }
  }

  fn _try_collect_from_cookies(req: &Request) -> Option<TokenTriple> {
    let access = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_collect_cookies(req, lbrp_types::LBRP_ACCESS);
    if access.is_empty() {
      return None;
    }

    let refresh = if let Some(rft) = req.cookies().get(lbrp_types::LBRP_REFRESH) {
      rft.value().to_string()
    } else {
      return None;
    };

    let cba = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_collect_cookies(req, lbrp_types::LBRP_CLIENT);
    let cba = if cba.is_empty() { None } else { Some(cba) };
    Some(TokenTriple { access, refresh, cba })
  }

  fn _try_collect(req: &Request) -> Result<TokenTriple, C3AClientError> {
    if let Some(triple) = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_try_collect_from_cookies(req) {
      Ok(triple)
    } else {
      Err(C3AClientError::BadUserRequest)
    }
  }

  fn deploy_triple_to_cookies(triple: &TokenTriple, res: &mut Response) {
    <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_deploy_cookie(res, lbrp_types::LBRP_ACCESS, &triple.access);
    <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_deploy_cookie(res, lbrp_types::LBRP_REFRESH, &triple.refresh);
    if let Some(cba) = &triple.cba {
      <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_deploy_cookie(res, lbrp_types::LBRP_CLIENT, cba);
    }
  }

  /// Just checks if a user is authenticated.
  async fn check_signed_in(&self, req: &mut Request, res: &mut Response) -> MResult<c3a_server_sdk::AuthorizeResponse> {
    <c3a_server_sdk::C3AClient as LbrpAuthMethods>::check_authorized_to(self, req, res, &[]).await
  }

  /// Checks if a user is authorized to given action tags.
  async fn check_authorized_to(
    &self,
    req: &mut Request,
    res: &mut Response,
    tags: &[AppTag],
  ) -> MResult<c3a_server_sdk::AuthorizeResponse> {
    let triple = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_try_collect(req)
      .map_err(|e| ServerError::from_private(e).with_401())?;

    let cba_challenge_state = req.header::<String>(lbrp_types::LBRP_CHALLENGE_STATE);
    let cba_challenge_sign = req
      .header::<String>(lbrp_types::LBRP_CHALLENGE_SIGN)
      .and_then(|v| c3a_common::base64_decode(&v).ok());

    let (triple, new_challenge_state) = self
      .authorize(triple, cba_challenge_state, cba_challenge_sign, tags)
      .await
      .map_err(|e| ServerError::from_private(e).with_401())?;

    if let Some(new_challenge_state) = new_challenge_state {
      res
        .add_header(lbrp_types::LBRP_CHALLENGE_STATE, new_challenge_state, true)
        .unwrap();
    }

    if let Some(new_challenge) = triple.new_cba_challenge {
      res
        .add_header(
          lbrp_types::LBRP_CHALLENGE,
          c3a_common::base64_encode(&new_challenge),
          true,
        )
        .unwrap();
    }

    if let Some(new_access_token) = &triple.new_access_token {
      Self::_deploy_cookie(res, lbrp_types::LBRP_ACCESS, new_access_token);
    }

    if let Some(new_cba_token) = &triple.new_cba_token {
      Self::_deploy_cookie(res, lbrp_types::LBRP_CLIENT, new_cba_token);
    }

    Ok(c3a_server_sdk::AuthorizeResponse {
      authorized: triple.approved,
    })
  }

  /// Forces to update client token.
  async fn update_client_token(&self, req: &mut Request, res: &mut Response) -> MResult<OK> {
    let triple = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_try_collect(req)
      .map_err(|e| ServerError::from_private(e).with_401())?;

    let cba_challenge_state = req.header::<String>(lbrp_types::LBRP_CHALLENGE_STATE);
    let cba_challenge_sign = req
      .header::<String>(lbrp_types::LBRP_CHALLENGE_SIGN)
      .and_then(|v| c3a_common::base64_decode(&v).ok());

    let (triple, new_challenge_state) = self
      .request_cba_update(triple, cba_challenge_state, cba_challenge_sign)
      .await
      .map_err(|e| ServerError::from_private(e).with_401())?;

    if let Some(new_challenge_state) = new_challenge_state {
      res
        .add_header(lbrp_types::LBRP_CHALLENGE_STATE, new_challenge_state, true)
        .unwrap();
    }

    if let Some(new_challenge) = triple.new_cba_challenge {
      res
        .add_header(
          lbrp_types::LBRP_CHALLENGE,
          c3a_common::base64_encode(&new_challenge),
          true,
        )
        .unwrap();
    }

    if let Some(new_access_token) = &triple.new_access_token {
      Self::_deploy_cookie(res, lbrp_types::LBRP_ACCESS, new_access_token);
    }

    if let Some(new_cba_token) = &triple.new_cba_token {
      Self::_deploy_cookie(res, lbrp_types::LBRP_CLIENT, new_cba_token);
    }

    ok!()
  }

  /// Requests a logout.
  ///
  /// After this, refresh token is no longer available, and access token stays alive up to 15 minutes.
  #[allow(dead_code)]
  async fn logout(&self, req: &mut Request, res: &mut Response) -> MResult<()> {
    let triple = <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_try_collect(req)
      .map_err(|e| ServerError::from_private(e).with_401())?;
    self
      .perform_logout(triple.access, triple.refresh)
      .await
      .map_err(|e| ServerError::from_private(e).with_500())?;

    <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_remove_cookies(res, lbrp_types::LBRP_ACCESS);
    res.cookies_mut().remove(lbrp_types::LBRP_REFRESH);
    <c3a_server_sdk::C3AClient as LbrpAuthMethods>::_remove_cookies(res, lbrp_types::LBRP_CLIENT);

    Ok(())
  }
}
