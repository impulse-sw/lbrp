use cc_server_kit::prelude::*;
use serde::{Deserialize, Serialize};
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
pub(crate) enum Service {
  #[cfg(feature = "err-handler")]
  ErrorHandler(ErrorHandler),
  CommonService(CommonService),
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
  pub(crate) from: String,
  pub(crate) to: String,
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
    Ok(child)
  }
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct LbrpConfig {
  pub(crate) lbrp_mode: LbrpMode,
  pub(crate) services: Vec<Service>,
}

impl LbrpConfig {
  #[cfg(feature = "err-handler")]
  pub(crate) fn validate(&self) -> MResult<()> {
    #[cfg(feature = "err-handler")]
    if !self.services.iter().any(|s| matches!(s, Service::ErrorHandler(_))) {
      return Err(
        ErrorResponse::from("There is no error handler service for LBRP installed")
          .with_500_pub()
          .build(),
      );
    }
    if let Some(invalid) = self
      .services
      .iter()
      .filter_map(|s| match s {
        Service::CommonService(service) => Some(service),
        #[allow(unreachable_patterns)]
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
