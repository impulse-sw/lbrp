#![deny(warnings, clippy::todo, clippy::unimplemented)]
#![allow(non_snake_case)]

use impulse_ui_kit::prelude::*;
use impulse_ui_kit::router::{get_path, redirect};
use lbrp_cli_authorize::CBAChallengeSign;

mod requests;

fn main() {
  let log_level = if cfg!(debug_assertions) {
    log::Level::Debug
  } else {
    log::Level::Info
  };
  setup_app(log_level, Box::new(move || view! { <AuthApp /> }.into_any()))
}

#[component]
fn FullscreenLoadingBox() -> impl IntoView {
  view! {
    <div class="flex mx-auto justify-center items-center h-screen w-2/5">
      <button class="border border-[#607a79] shadow rounded-md p-2 flex flex-row items-center gap-2 text-gray-600 hover:text-black cursor-pointer">
        <Icon class="w-5 h-5 animate-spin" icon=icondata::LuLoader2 />
        <p>"Загрузка..."</p>
      </button>
    </div>
  }
}

#[component]
fn AuthApp() -> impl IntoView {
  let authorized = LocalResource::new(crate::requests::check_auth);

  let page = RwSignal::new(String::new());
  Effect::new(move |_| {
    if let Some(auth) = authorized.get() {
      if !*auth {
        *page.write() = "login".to_string();
      } else {
        redirect(get_path().unwrap()).unwrap();
      }
    }
  });

  view! {
    <Show when=move || page.read().as_str().eq("")>
      <FullscreenLoadingBox />
    </Show>
    <Show when=move || page.read().as_str().eq("login")>
      <LoginPage authorized />
    </Show>
  }
}

#[component]
pub(crate) fn LoginPage(authorized: LocalResource<bool>) -> impl IntoView {
  let login = RwSignal::new(String::new());
  let password = RwSignal::new(String::new());
  // let err_msg = RwSignal::new(String::new());

  let login_triggered = RwSignal::new(false);
  let sign_up_triggered = RwSignal::new(false);

  let sign_up_resource = LocalResource::new(move || async move {
    if *sign_up_triggered.read() {
      let login = (*login.read()).clone();
      let password = (*password.read()).clone();

      let (state, challenge) = if let Ok((state, challenge)) = crate::requests::sign_up_step1(login.clone()).await {
        (state, challenge)
      } else {
        *sign_up_triggered.write() = false;
        return None;
      };

      let keyring = lbrp_cli_authorize::client_keypair().unwrap();
      let challenge_sign = CBAChallengeSign::new(keyring.sign_raw(&challenge));

      if crate::requests::sign_up_step2(login, password, state, keyring.public(), challenge_sign)
        .await
        .is_err()
      {
        *sign_up_triggered.write() = false;
        return None;
      }

      authorized.refetch();

      Some(())
    } else {
      None
    }
  });

  let sign_up_task = move |_| {
    *sign_up_triggered.write_untracked() = true;
    sign_up_resource.refetch();
  };

  let login_resource = LocalResource::new(move || async move {
    if *login_triggered.read() {
      let login = (*login.read()).clone();
      let password = (*password.read()).clone();

      let challenge = if let Ok(challenge) = crate::requests::login_step1(login.clone()).await {
        challenge
      } else {
        *login_triggered.write() = false;
        return None;
      };

      let keyring = lbrp_cli_authorize::client_keypair().unwrap();
      let challenge_sign = CBAChallengeSign::new(keyring.sign_raw(&challenge));

      if crate::requests::login_step2(login, password, keyring.public(), challenge_sign)
        .await
        .is_err()
      {
        *login_triggered.write() = false;
        return None;
      }

      authorized.refetch();

      Some(())
    } else {
      None
    }
  });

  let login_task = move |_| {
    *login_triggered.write_untracked() = true;
    login_resource.refetch();
  };

  view! {
    <div class="flex flex-col items-center justify-center h-full w-full bg-gray-100 dark:bg-gray-900">
      <div class="flex flex-col items-center justify-center min-h-screen w-2/5">
        <p class="mb-4 text-xl text-gray-600 dark:text-gray-300 text-center">
          Сервисы Импульса
        </p>
        <Space vertical=true>
          <Input
            value=login
            input_type=InputType::Text
            placeholder="Имя пользователя"
          />
          <Input value=password input_type=InputType::Password placeholder="Пароль" />
          <Button block=true appearance=ButtonAppearance::Primary on_click=login_task>
            "Войти"
          </Button>
          <Button block=true appearance=ButtonAppearance::Secondary on_click=sign_up_task>
            "Зарегистрироваться"
          </Button>
        </Space>
      </div>
    </div>
  }
}
