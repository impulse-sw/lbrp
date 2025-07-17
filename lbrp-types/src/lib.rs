use c3a_common::CBAChallengeSign;
use serde::{Deserialize, Serialize};

pub const LBRP_ACCESS: &str = "LBRP-Access";
pub const LBRP_REFRESH: &str = "LBRP-Refresh";
pub const LBRP_CLIENT: &str = "LBRP-Client";
pub const LBRP_CHALLENGE: &str = "LBRP-Challenge";
pub const LBRP_CHALLENGE_STATE: &str = "LBRP-Challenge-State";
pub const LBRP_CHALLENGE_SIGN: &str = "LBRP-Challenge-Sign";

#[derive(Serialize, Deserialize, Clone)]
pub struct LoginRequest {
  pub id: String,
  pub password: String,
  pub cdpub: Option<Vec<u8>>,
  pub cba_challenge_sign: Option<CBAChallengeSign>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LoginResponse {
  pub challenge: Option<Vec<u8>>,
}

pub type RegisterRequest = LoginRequest;
pub type RegisterResponse = LoginResponse;
