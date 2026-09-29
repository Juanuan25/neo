//! Per-service appdata snapshots (option pane).
//!
//! The appdata folder usually shares a dataset with every other service
//! (`zroot/neo` → /var/neo), so `zfs rollback` is not an option: it would
//! roll back all services and destroy newer snapshots. Restore instead:
//! pre-restore snapshot → stop `neo-<svc>.target` + units → rsync the folder
//! back from `.zfs/snapshot/<snap>/<rel>` → start what was running.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command as AsyncCommand;

use super::super::types::AppConfig;
use super::super::units::{
    broadcast_unit_update, end_clear_appdata, schedule_unit_refresh_burst, start_units_best_effort,
    systemctl_action_blocking, unit_active_state_async, units_currently_running,
    wait_units_stopped,
};
use super::super::util::{
    escape_attr, escape_html, status_err, status_ok, status_pulling, status_slot_oob, sudo_cmd,
};
use super::render::{self, BTN_SECONDARY};
use super::{
    create_snapshot, dataset_for_path, format_epoch_utc, list_snapshots, now_epoch, now_ts,
    rsync_bin, snapshot_has_path, zfs_mounts, Snapshot, ZfsMount,
};

/// Units the restore must never stop: stopping them kills the web UI running the job.
/// Same exclusions as the service targets (nix/lib/service-targets.nix).
const NEVER_STOP: &[&str] = &["neo-web", "neo-bootstrap"];

const OUT_CLASSES: &str = "snapshot-out text-xs max-w-full truncate empty:hidden";

/// Most rows shown in the pane (newest first).
const MAX_ROWS: usize = 60;

pub fn snapshot_out_oob(service: &str, inner: &str, title: &str) -> String {
    status_slot_oob("snapshot-out", service, OUT_CLASSES, inner, title)
}

/// Where a service's appdata lives on ZFS.
pub struct AppdataLocation {
    pub mount: ZfsMount,
    /// Path of the appdata folder relative to the dataset mountpoint.
    pub rel: String,
}

pub fn locate_appdata(appdata: &str) -> Option<AppdataLocation> {
    let (mount, rel) = dataset_for_path(&zfs_mounts(), appdata)?;
    Some(AppdataLocation { mount, rel })
}

/// Snapshot name for a manual per-service snapshot.
pub fn manual_snapshot_name(service: &str) -> String {
    format!("neo-snap-{}-{}", service, now_ts())
}

fn prerestore_snapshot_name(service: &str) -> String {
    format!("neo-prerestore-{}-{}", service, now_ts())
}

/// The whole snapshots card for a service. `oob` wraps it for a WS push.
/// Same layout and rows as the versioning tab's data snapshots ([`super::render`]).
pub async fn render_service_snapshots(
    service: &str,
    appdata: &str,
    busy: bool,
    oob: bool,
) -> String {
    let svc = escape_html(service);
    let oob_attr = if oob { r#" hx-swap-oob="true""# } else { "" };
    let open = format!(r#"<div id="snapshots-{svc}" class="space-y-3"{oob_attr}>"#);
    let unavailable = |msg: &str| {
        format!(
            r#"{open}<div class="rounded-box border border-base-300 bg-base-100 px-4 py-6 text-center text-xs text-base-content/50">{}</div></div>"#,
            escape_html(msg)
        )
    };

    let Some(loc) = locate_appdata(appdata) else {
        return unavailable(
            "Snapshots are unavailable: this service's app data is not on a ZFS dataset.",
        );
    };
    if loc.rel.is_empty() {
        return unavailable("This service's app data is a whole dataset — use Data snapshots in the versioning tab.");
    }

    let snaps = list_snapshots(&loc.mount.dataset, false).await;
    let ds = escape_html(&loc.mount.dataset);
    let (dis_cls, dis_attr) = if busy {
        (" btn-disabled", " disabled")
    } else {
        ("", "")
    };

    let mut html = open;
    html.push_str(&format!(
        r##"<div class="flex flex-wrap items-center gap-2">
  <p class="text-xs sm:text-sm text-base-content/60 min-w-0 flex-1">Snapshots of this service's app data <span class="font-mono break-all">{appdata_html}</span>.</p>
  <span class="flex gap-1.5">
    <button type="button" class="{BTN_SECONDARY}" hx-get="/service/{svc}/snapshots" hx-target="#snapshots-{svc}" hx-swap="outerHTML">Refresh</button>
    <button type="button" class="btn btn-sm btn-primary{dis_cls}"{dis_attr} hx-post="/service/{svc}/snapshots/create" hx-swap="none" hx-disabled-elt="this" title="zfs snapshot {ds}@neo-snap-{svc}-…">Snapshot now</button>
  </span>
</div>
<div id="snapshot-out-{svc}" class="{OUT_CLASSES}" title=""></div>
{how}"##,
        appdata_html = escape_html(appdata),
        how = render::how_it_works(&[
            "The service is <b>stopped</b> while its app data folder is copied back from the snapshot, then started again.",
            "Only this service's folder is replaced; other services and the activated system are not touched.",
            "A <span class=\"badge badge-warning badge-xs\">pre-restore</span> snapshot is taken first, so a restore can be undone from this list.",
        ]),
    ));

    match snaps {
        Err(e) => {
            html.push_str(&format!(
                r#"<div role="alert" class="alert alert-soft alert-error py-2 px-3 text-xs">{}</div>"#,
                escape_html(&e)
            ));
        }
        Ok(list) => {
            let now = now_epoch();
            let rows: Vec<String> = list
                .iter()
                .rev()
                .take(MAX_ROWS)
                .map(|s| snapshot_row(service, appdata, &loc, s, now, busy))
                .collect();
            html.push_str(&render::snapshot_list(
                "Snapshots",
                &loc.mount.dataset,
                true,
                list.len(),
                &rows,
                MAX_ROWS,
            ));
        }
    }
    html.push_str("</div>");
    html
}

fn snapshot_row(
    service: &str,
    appdata: &str,
    loc: &AppdataLocation,
    s: &Snapshot,
    now: i64,
    busy: bool,
) -> String {
    let when = format_epoch_utc(s.creation);
    let confirm = format!(
        "Restore {service} appdata to snapshot {name} ({when})?\n\n\
         • All units of {service} are stopped.\n\
         • {appdata} is REPLACED by the snapshot copy: changes and files newer than the snapshot are lost.\n\
         • A pre-restore snapshot of {ds} is taken first, so this can be undone from this list.\n\
         • Other services and the activated system are not touched.",
        name = s.name,
        ds = loc.mount.dataset,
    );
    let action = if busy {
        r#"<button type="button" class="btn btn-sm btn-disabled" disabled title="Another app data operation is running">Restore</button>"#
            .to_string()
    } else {
        format!(
            r#"<button type="button" class="{BTN_SECONDARY}" hx-post="/service/{svc}/snapshots/restore?snap={snap_q}" hx-swap="none" hx-disabled-elt="this" hx-confirm="{confirm}" title="Replace the app data folder with this snapshot">Restore</button>"#,
            svc = escape_attr(service),
            snap_q = escape_attr(&urlencode(&s.name)),
            confirm = escape_attr(&confirm),
        )
    };
    let manual_mine = s.name.starts_with(&format!("neo-snap-{service}-"))
        || s.name.starts_with(&format!("neo-prerestore-{service}-"));
    render::snapshot_row(s, now, "", &action, manual_mine)
}

/// Minimal query escaping for snapshot names (validated charset plus `:`).
fn urlencode(s: &str) -> String {
    s.replace('%', "%25").replace(':', "%3A")
}

async fn rsync_restore(src: &Path, dst: &str) -> Result<(), String> {
    let mut src_s = src.to_string_lossy().into_owned();
    src_s.push('/');
    let mut dst_s = dst.trim_end_matches('/').to_string();
    dst_s.push('/');
    let out = AsyncCommand::new(sudo_cmd())
        .args([
            "-n",
            &rsync_bin(),
            "-aHX",
            "--delete",
            "--numeric-ids",
            "--",
            &src_s,
            &dst_s,
        ])
        .output()
        .await
        .map_err(|e| format!("rsync: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        Err(format!("rsync failed: {}", last.trim()))
    }
}

pub struct RestoreJob {
    pub service: String,
    pub appdata: String,
    pub snap: String,
    pub units: Vec<String>,
    pub group_unit: Option<String>,
}

/// Background restore. The caller holds the service's in-flight slot
/// (shared with clear-appdata); this releases it.
pub async fn run_restore(job: RestoreJob, config: Arc<AppConfig>) {
    let result = restore_inner(&job, &config).await;
    let (inner, title) = match &result {
        Ok(msg) => status_ok(msg),
        Err(e) => status_err(e),
    };
    let _ = config
        .unit_updates
        .send(snapshot_out_oob(&job.service, &inner, &title));
    end_clear_appdata(&config, &job.service);
    let card = render_service_snapshots(&job.service, &job.appdata, false, true).await;
    let _ = config.unit_updates.send(card);
    // The card re-render clears the status slot; push the result again after it.
    let _ = config
        .unit_updates
        .send(snapshot_out_oob(&job.service, &inner, &title));
    for u in &job.units {
        broadcast_unit_update(u, &config);
        schedule_unit_refresh_burst(u.clone(), Arc::clone(&config));
    }
}

async fn restore_inner(job: &RestoreJob, config: &Arc<AppConfig>) -> Result<String, String> {
    let push = |msg: &str| {
        let (inner, title) = status_pulling(msg);
        let _ = config
            .unit_updates
            .send(snapshot_out_oob(&job.service, &inner, &title));
    };

    let loc = locate_appdata(&job.appdata).ok_or("appdata is not on a ZFS dataset")?;
    if loc.rel.is_empty() {
        return Err("appdata is a whole dataset; refusing".to_string());
    }
    let snaps = list_snapshots(&loc.mount.dataset, false).await?;
    if !snaps.iter().any(|s| s.name == job.snap) {
        return Err(format!("snapshot {} not found", job.snap));
    }
    if snapshot_has_path(&loc.mount, &job.snap, &loc.rel) == Some(false) {
        return Err(format!(
            "{} did not exist yet in {} (use Clear appdata instead)",
            job.appdata, job.snap
        ));
    }
    let src = Path::new(&loc.mount.mountpoint)
        .join(".zfs/snapshot")
        .join(&job.snap)
        .join(&loc.rel);

    push("taking pre-restore snapshot…");
    let safety =
        create_snapshot(&loc.mount.dataset, &prerestore_snapshot_name(&job.service)).await?;

    let units: Vec<String> = job
        .units
        .iter()
        .filter(|u| !NEVER_STOP.contains(&u.as_str()))
        .cloned()
        .collect();
    let to_restart = units_currently_running(&units).await;
    let target_was_active = match &job.group_unit {
        Some(t) => matches!(
            unit_active_state_async(t).await.as_str(),
            "active" | "activating" | "reloading"
        ),
        None => false,
    };

    push("stopping service…");
    if let Some(t) = &job.group_unit {
        if let Err(e) = systemctl_action_blocking("stop", t).await {
            eprintln!("web: snapshot restore stop {t}: {e}");
        }
    }
    for u in &units {
        if let Err(e) = systemctl_action_blocking("stop", u).await {
            eprintln!("web: snapshot restore stop {u}: {e}");
        }
        broadcast_unit_update(u, config);
    }

    let restart = |config: Arc<AppConfig>| {
        let to_restart = to_restart.clone();
        let group = job.group_unit.clone();
        async move {
            if let (Some(t), true) = (group, target_was_active) {
                if let Err(e) = systemctl_action_blocking("start", &t).await {
                    eprintln!("web: snapshot restore start {t}: {e}");
                }
            }
            start_units_best_effort(&to_restart, &config).await;
        }
    };

    if let Err(e) = wait_units_stopped(&units, Duration::from_secs(120)).await {
        restart(Arc::clone(config)).await;
        return Err(e);
    }

    push(&format!("restoring files from {}…", job.snap));
    if let Err(e) = rsync_restore(&src, &job.appdata).await {
        restart(Arc::clone(config)).await;
        return Err(format!(
            "{e} — appdata may be partial; restore {safety} to undo"
        ));
    }

    if target_was_active || !to_restart.is_empty() {
        push("starting service…");
    }
    restart(Arc::clone(config)).await;

    Ok(format!(
        "restored {} (undo: pre-restore snapshot)",
        job.snap
    ))
}
