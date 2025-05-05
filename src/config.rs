use cc_server_kit::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Default)]
pub(crate) enum LbrpMode {
  /// Сам принимает все соединения и управляет сервисами.
  #[default]
  Single,
  /// Не принимает соединений и не управляет сервисами; управляет доставкой сервисов и настройкой нодов.
  Supervisor,
  /// Модель ребёнка и родителя
  PC(LbrpPCMode),
  /// Модель братьев
  Ybob(LbrpYBOBMode),
}

#[derive(Deserialize, Serialize)]
pub(crate) enum LbrpPCMode {
  /// Родитель: принимает все соединения и перенаправляет их детям. Просто балансировщик нагрузки.
  Parent,
  /// Ребёнок: перенаправляет соединения от родителя к сервисам и управляет сервисами. Просто реверс-прокси.
  Child,
}

#[derive(Deserialize, Serialize)]
pub(crate) enum LbrpYBOBMode {
  /// Старший брат: любит скидывать всю работу на младших братьев, но также владеет сервисами на случай, если младших братьев не будет рядом.
  OlderBrother,
  /// Младший брат: просто реверс-прокси.
  YoungerBrother,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Service {
  ErrorHandler(ErrorHandler),
  CommonService(CommonService),
  CommonStatic(CommonStatic),
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct ErrorHandler {
  pub(crate) dist_dir: PathBuf,
  pub(crate) static_files: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CommonService {
  #[cfg(feature = "c3a")]
  pub(crate) service_name: String,
  #[cfg(feature = "c3a")]
  pub(crate) require_subdomain_access_token: Option<bool>,
  pub(crate) startup_cmd: Option<PathBuf>,
  pub(crate) working_dir: Option<PathBuf>,
  pub(crate) wait_after: Option<u64>,
  pub(crate) from: String,
  pub(crate) to: String,
  pub(crate) cors_domains: Option<Vec<String>>,
  pub(crate) skip_err_handling: Option<bool>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CommonStatic {
  pub(crate) static_routes: HashMap<String, PathBuf>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub(crate) struct CorsOpts {
  pub(crate) allowed_methods: String,
  pub(crate) allowed_client_headers: String,
  pub(crate) allowed_headers: String,
}

impl Default for CorsOpts {
  fn default() -> Self {
    Self {
      allowed_methods: "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS".to_string(),
      allowed_client_headers: "Authorization, Set-Cookie, C3A-Registration-State, C3A-Challenge-State, C3A-Access, C3A-Client".to_string(),
      allowed_headers: "Authorization, Accept, Access-Control-Allow-Headers, Origin, X-Requested-With, Content-Type, Cookie, Set-Cookie, C3A-Registration-State, C3A-Challenge-State, C3A-Access, C3A-Client".to_string(),
    }
  }
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct LbrpConfig {
  pub(crate) lbrp_mode: LbrpMode,
  pub(crate) services: Vec<Service>,
  pub(crate) cors_opts: CorsOpts,
}

impl CommonService {
  pub(crate) fn should_startup(&self) -> bool {
    self.startup_cmd.is_some() && self.working_dir.is_some()
  }

  pub(crate) fn startup(&self) -> MResult<std::process::Child> {
    use std::process::Command;

    let startup_cmd = self.startup_cmd.as_ref().ok_or(
      ErrorResponse::from("There is no `startup_cmd` specified!")
        .with_500_pub()
        .build(),
    )?;
    let working_dir = self.working_dir.as_ref().ok_or(
      ErrorResponse::from("There is no `working_dir` specified!")
        .with_500_pub()
        .build(),
    )?;

    let child = Command::new(startup_cmd)
      .current_dir(working_dir)
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::piped())
      .spawn()
      .map_err(|e| ErrorResponse::from(e).with_500_pub().build())?;

    if let Some(wait_after) = self.wait_after {
      std::thread::sleep(std::time::Duration::from_secs(wait_after));
    }

    Ok(child)
  }
}

impl LbrpConfig {
  pub(crate) fn validate(&self) -> MResult<()> {
    if let Some(invalid) = self
      .services
      .iter()
      .filter_map(|s| match s {
        Service::CommonService(service) => Some(service),
        _ => None,
      })
      .find(|s| !s.to.starts_with("http://") && !s.to.starts_with("https://"))
    {
      return Err(
        ErrorResponse::from(format!(
          "You aren't specified what schema (`http` or `https`) LBRP must use with `{}`",
          invalid.to
        ))
        .with_500_pub()
        .build(),
      );
    }
    Ok(())
  }
}

pub(crate) async fn config_watcher<P: AsRef<std::path::Path>>(
  config_path: P,
  reload_tx: tokio::sync::broadcast::Sender<()>,
) -> notify::Result<()> {
  use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

  let (tx, mut rx) = tokio::sync::mpsc::channel(1);
  let mut watcher = RecommendedWatcher::new(move |res| tx.blocking_send(res).unwrap(), Config::default())?;
  watcher.watch(config_path.as_ref(), RecursiveMode::NonRecursive)?;

  while let Some(res) = rx.recv().await {
    match res {
      Ok(event) if event.kind.is_modify() => {
        let _ = reload_tx.send(());
      }
      Err(e) => println!("Watch error: {:?}", e),
      _ => {}
    }
  }

  Ok(())
}
