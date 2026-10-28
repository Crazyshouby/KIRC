//! Updates from the GitHub releases.
//!
//! The release workflow signs each installer with a private key that only GitHub Actions holds
//! (the "TAURI_SIGNING_PRIVATE_KEY" secret) and publishes `latest.json` next to it. This program
//! carries the matching public key (tauri.conf.json, plugins.updater.pubkey) and installs nothing
//! whose signature does not verify against it: a tampered download, or a release made by someone
//! who does not hold the key, is refused. The check only asks GitHub for `latest.json`.

use crate::app::{self, App};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// What the pages are told about an update.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UpdateEvent {
    Available { info: UpdateInfo },
    /// The check found nothing newer.
    None,
    Progress { done: u64, total: u64 },
    Installing,
    Error { message: String },
}

#[derive(Default)]
pub struct Updates {
    pending: tokio::sync::Mutex<Option<Update>>,
    info: std::sync::Mutex<Option<UpdateInfo>>,
    busy: AtomicBool,
}

impl Updates {
    pub fn info(&self) -> Option<UpdateInfo> {
        self.info.lock().unwrap().clone()
    }
}

fn tell(app: &AppHandle, ev: UpdateEvent) {
    let _ = app.emit("update", ev);
}

/// Releases `busy` when dropped, whatever way the function leaves.
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn take_busy(flag: &AtomicBool) -> Result<Busy<'_>, String> {
    if flag.swap(true, Ordering::AcqRel) {
        Err("Une vérification ou une installation est déjà en cours".into())
    } else {
        Ok(Busy(flag))
    }
}

/// Asks GitHub whether a newer release exists. `manual` only changes what is announced:
/// a manual check also says when there is nothing new.
pub async fn check(app: &AppHandle, manual: bool) -> Result<Option<UpdateInfo>, String> {
    let st = app.state::<App>();
    let _busy = take_busy(&st.updates.busy)?;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => {
            let info = UpdateInfo {
                version: update.version.clone(),
                current: update.current_version.clone(),
                notes: update.body.clone().unwrap_or_default().chars().take(2000).collect(),
            };
            *st.updates.pending.lock().await = Some(update);
            *st.updates.info.lock().unwrap() = Some(info.clone());
            tell(app, UpdateEvent::Available { info: info.clone() });
            Ok(Some(info))
        }
        None => {
            *st.updates.pending.lock().await = None;
            *st.updates.info.lock().unwrap() = None;
            if manual {
                tell(app, UpdateEvent::None);
            }
            Ok(None)
        }
    }
}

/// Downloads the update (the signature is verified there), leaves the servers politely, then
/// runs the installer, which closes and restarts the application.
pub async fn install(app: AppHandle) -> Result<(), String> {
    let st = app.state::<App>();
    let _busy = take_busy(&st.updates.busy)?;
    let update = st.updates.pending.lock().await.take().ok_or("Aucune mise à jour à installer : lancez d'abord une vérification")?;

    let mut done: u64 = 0;
    let mut last_percent = 101;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let total = total.unwrap_or(0);
                let percent = (done * 100).checked_div(total).unwrap_or(0);
                if percent != last_percent {
                    last_percent = percent;
                    tell(&app, UpdateEvent::Progress { done, total });
                }
            },
            || {},
        )
        .await
        .map_err(|e| {
            let message = format!("Téléchargement ou vérification de la signature impossible : {e}");
            tell(&app, UpdateEvent::Error { message: message.clone() });
            message
        })?;

    tell(&app, UpdateEvent::Installing);
    app::leave_servers(&app).await;
    if let Err(e) = update.install(bytes) {
        // The servers are gone but the application is not: say so, and let the user retry.
        st.reopen_after_failed_exit();
        let message = format!("Installation impossible : {e}");
        tell(&app, UpdateEvent::Error { message: message.clone() });
        *st.updates.pending.lock().await = Some(update);
        return Err(message);
    }
    Ok(())
}

/// Checks a few seconds after the start, then every six hours, as long as the setting is on.
pub fn spawn_background(app: &AppHandle) {
    if std::env::var_os("KIRC_NO_UPDATE_CHECK").is_some() || cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let enabled = app.state::<App>().config.lock().unwrap().settings.check_updates;
            if enabled {
                if let Err(e) = check(&app, false).await {
                    log::info!("recherche de mise à jour : {e}");
                }
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}
