use serde::{Deserialize, Serialize};

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
pub(crate) struct Service {
  pub(crate) from: String,
  pub(crate) to: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct LbrpConfig {
  pub(crate) lbrp_mode: LbrpMode,
  pub(crate) services: Vec<Service>,
}

impl Default for LbrpConfig {
  fn default() -> Self {
    Self {
      lbrp_mode: LbrpMode::default(),
      services: vec![Service { from: "127.0.0.1:8801".into(), to: "127.0.0.1:8001".into() }],
    }
  }
}

pub(crate) async fn config_watcher<P: AsRef<std::path::Path>>(
  config_path: P,
  reload_tx: tokio::sync::broadcast::Sender<()>,
) -> notify::Result<()> {
  use notify::{Config, Watcher, RecommendedWatcher, RecursiveMode};
  
  let (tx, mut rx) = tokio::sync::mpsc::channel(1);
  
  let mut watcher = RecommendedWatcher::new(
    move |res| tx.blocking_send(res).unwrap(),
    Config::default(),
  )?;

  watcher.watch(config_path.as_ref(), RecursiveMode::NonRecursive)?;

  while let Some(res) = rx.recv().await {
    match res {
      Ok(event) if event.kind.is_modify() => { let _ = reload_tx.send(()); },
      Err(e) => println!("Watch error: {:?}", e),
      _ => {},
    }
  }

  Ok(())
}
