use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct LoginRequest {
  pub id: String,
  pub password: String,
  pub cdpub: Option<Vec<u8>>,
  pub cba_challenge_sign: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LoginResponse {
  pub challenge: Option<Vec<u8>>,
}

pub type RegisterRequest = LoginRequest;
pub type RegisterResponse = LoginResponse;
