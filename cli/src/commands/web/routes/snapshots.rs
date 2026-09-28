//! ZFS snapshot routes: per-service appdata (option pane) and all Neo data (versioning tab).
use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{get, post, State};

use crate::commands::web::structs::{AppConfig, OptionPaneContext};
use crate::commands::web::units::{is_safe_appdata_path, try_begin_clear_appdata};
use crate::commands::web::util::{
    escape_html, service_name_ok, status_err, status_ok, status_pulling,
};
use crate::commands::web::zfs::service::{
    locate_appdata, manual_snapshot_name, render_service_snapshots, run_restore, snapshot_out_oob,
    RestoreJob,
};
use crate::commands::web::zfs::{create_snapshot, data, snapshot_name_ok};

/// Trusted appdata path for a service (from flake evaluation, never the client).
async fn service_pane(
    service: &str,
    config: &State<Arc<AppConfig>>,
) -> Result<(OptionPaneContext, String), String> {
    let pane = {
        let mut ev = config.evaluator.lock().await;
        ev.extract_service_options(service).await
    };
    if let Some(err) = pane.eval_error.error.as_ref() {
        return Err(format!("eval failed: {err}"));
    }
    let appdata = match pane.appdata.as_ref() {
        Some(p) if !p.is_empty() => p.clone(),
        _ => return Err("no appdata path declared for this service".to_string()),
    };
    if !is_safe_appdata_path(&appdata, pane.appdata_root.as_deref()) {
        return Err(format!("refusing unsafe appdata path: {appdata}"));
    }
    Ok((pane, appdata))
}

fn out_err(service: &str, msg: &str) -> RawHtml<String> {
    let (inner, title) = status_err(msg);
    RawHtml(snapshot_out_oob(service, &inner, &title))
}

/// Snapshot card for the option pane (lazy-loaded).
#[get("/service/<service>/snapshots")]
pub async fn service_snapshots(service: &str, config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    if !service_name_ok(service) {
        return RawHtml(String::new());
    }
    match service_pane(service, config).await {
        Ok((_, appdata)) => {
            let busy = config.clear_appdata_in_flight.contains(service);
            RawHtml(render_service_snapshots(service, &appdata, busy, false).await)
        }
        Err(e) => RawHtml(format!(
            r#"<div id="snapshots-{}" class="text-[10px] text-error mb-2">{}</div>"#,
            escape_html(service),
            escape_html(&e)
        )),
    }
}

/// Manual snapshot of the dataset holding the service's appdata.
#[post("/service/<service>/snapshots/create")]
pub async fn service_snapshot_create(
    service: &str,
    config: &State<Arc<AppConfig>>,
) -> RawHtml<String> {
    if !service_name_ok(service) {
        return RawHtml(String::new());
    }
    let appdata = match service_pane(service, config).await {
        Ok((_, a)) => a,
        Err(e) => return out_err(service, &e),
    };
    let Some(loc) = locate_appdata(&appdata) else {
        return out_err(service, "appdata is not on a ZFS dataset");
    };
    match create_snapshot(&loc.mount.dataset, &manual_snapshot_name(service)).await {
        Ok(full) => {
            let busy = config.clear_appdata_in_flight.contains(service);
            let card = render_service_snapshots(service, &appdata, busy, true).await;
            let (inner, title) = status_ok(&format!("created {full}"));
            RawHtml(format!(
                "{card}{}",
                snapshot_out_oob(service, &inner, &title)
            ))
        }
        Err(e) => out_err(service, &e),
    }
}

/// Restore the service's appdata folder from a snapshot (background job, WS progress).
#[post("/service/<service>/snapshots/restore?<snap>")]
pub async fn service_snapshot_restore(
    service: &str,
    snap: &str,
    config: &State<Arc<AppConfig>>,
) -> RawHtml<String> {
    if !service_name_ok(service) {
        return RawHtml(String::new());
    }
    if !snapshot_name_ok(snap) {
        return out_err(service, "invalid snapshot name");
    }
    let (pane, appdata) = match service_pane(service, config).await {
        Ok(v) => v,
        Err(e) => return out_err(service, &e),
    };
    // Shared with clear-appdata: never both at once on one service.
    if !try_begin_clear_appdata(config, service) {
        return out_err(service, "another appdata operation is in progress");
    }

    let job = RestoreJob {
        service: service.to_string(),
        appdata: appdata.clone(),
        snap: snap.to_string(),
        units: pane.units.iter().map(|u| u.name.clone()).collect(),
        group_unit: pane.group_unit.clone(),
    };
    let card = render_service_snapshots(service, &appdata, true, true).await;
    let (inner, title) = status_pulling("starting restore…");
    let out = snapshot_out_oob(service, &inner, &title);
    let _ = config.unit_updates.send(card.clone());
    let _ = config.unit_updates.send(out.clone());

    let cfg = Arc::clone(config);
    tokio::spawn(async move {
        run_restore(job, cfg).await;
    });
    RawHtml(format!("{card}{out}"))
}

/// Data snapshots card for the versioning tab (empty + hidden when unavailable).
#[get("/versioning/zfs")]
pub async fn versioning_zfs() -> RawHtml<String> {
    RawHtml(data::render_card(None).await)
}

#[post("/versioning/zfs/snapshot")]
pub async fn versioning_zfs_snapshot() -> RawHtml<String> {
    RawHtml(data::snapshot_now().await)
}

#[post("/versioning/zfs/restore?<ds>&<snap>&<gen>")]
pub async fn versioning_zfs_restore(ds: &str, snap: &str, gen: Option<u64>) -> RawHtml<String> {
    RawHtml(data::restore(ds, snap, gen).await)
}

#[post("/versioning/zfs/reboot")]
pub async fn versioning_zfs_reboot() -> RawHtml<String> {
    RawHtml(data::reboot_now().await)
}

#[post("/versioning/zfs/cancel")]
pub async fn versioning_zfs_cancel() -> RawHtml<String> {
    RawHtml(data::cancel().await)
}

#[post("/versioning/zfs/dismiss")]
pub async fn versioning_zfs_dismiss() -> RawHtml<String> {
    RawHtml(data::dismiss().await)
}
