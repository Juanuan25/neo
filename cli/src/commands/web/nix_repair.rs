// Long-running Nix store repair jobs triggered from the web UI.
// Does not hold the evaluator mutex while `nix-store` runs; refreshes the repl afterwards.
use std::fs;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::commands::web::ops::store::{
    append_log, find_recent_in_progress, log_path, write_state,
};
use crate::commands::web::types::AppConfig;
use crate::commands::web::util::{nix_bin, sudo_cmd};
use crate::utils::{get_timestamp, OPERATIONS_DIR};

/// Most recent in-progress repair within the last hour, if any.
pub fn find_recent_in_progress_repair() -> Option<String> {
    find_recent_in_progress("repair_")
}

/// Start a background store verify+repair. Returns the operation id.
/// Single-flight: if one is already running, returns that id instead of starting another.
pub fn start_store_verify_repair(config: Arc<AppConfig>) -> String {
    if let Some(existing) = find_recent_in_progress_repair() {
        return existing;
    }
    let ts = get_timestamp();
    let id = format!("repair_{ts}");
    let _ = fs::create_dir_all(OPERATIONS_DIR);
    write_state(&id, "in_progress", "starting", None);
    let _ = fs::write(
        log_path(&id),
        format!("{id} store verify/repair triggered via web at {ts}\n"),
    );

    let id_for_task = id.clone();
    tokio::spawn(async move {
        run_store_verify_repair(config, id_for_task).await;
    });
    id
}

async fn run_store_verify_repair(config: Arc<AppConfig>, id: String) {
    write_state(&id, "in_progress", "nix-store-verify-repair", None);
    let log_file = log_path(&id);
    let sudo = sudo_cmd();
    // Prefer the same nix binary neo-web uses when available.
    let nix_store = {
        let nix = nix_bin();
        let p = std::path::Path::new(&nix);
        if let Some(parent) = p.parent() {
            let candidate = parent.join("nix-store");
            if candidate.is_file() {
                candidate.to_string_lossy().into_owned()
            } else {
                "nix-store".to_string()
            }
        } else {
            "nix-store".to_string()
        }
    };

    // Use sudo -n (non-interactive). Do not pass --no-ask-password: that is a
    // systemctl flag, not a sudo option. Skip --check-contents (full content
    // rehash is very slow); path existence + repair is enough for missing store paths.
    let mut child = match TokioCommand::new(&sudo)
        .args(["-n", &nix_store, "--verify", "--repair"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("failed to spawn store repair: {e}");
            append_log(&log_file, &msg);
            write_state(&id, "failed", "spawn", Some(&msg));
            return;
        }
    };

    // Stream stdout+stderr into the log file.
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let log_out = log_file.clone();
    let log_err = log_file.clone();
    let out_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                append_log(&log_out, &line);
            }
        }
    });
    let err_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                append_log(&log_err, &line);
            }
        }
    });

    let status = child.wait().await;
    let _ = out_task.await;
    let _ = err_task.await;

    match status {
        Ok(st) if st.success() => {
            append_log(&log_file, "nix-store --verify --repair completed OK");
            write_state(&id, "in_progress", "refresh-repl", None);
            // Restart the persistent repl and warm extracts so the UI can recover.
            {
                let mut ev = config.evaluator.lock().await;
                match ev.refresh().await {
                    Ok(()) => {
                        append_log(&log_file, "nix repl refreshed after store repair");
                        let nav = ev.extract_proxied_services().await;
                        if let Some(err) = nav.eval_error.error.as_ref() {
                            append_log(&log_file, &format!("warm-up still reports error: {err}"));
                        } else {
                            append_log(&log_file, "warm-up navigator extract succeeded");
                        }
                        let _ = ev.extract_neo_theme().await;
                    }
                    Err(e) => {
                        append_log(&log_file, &format!("repl refresh failed: {e:#}"));
                        write_state(
                            &id,
                            "failed",
                            "refresh-repl",
                            Some(&format!("store repair OK but repl refresh failed: {e:#}")),
                        );
                        return;
                    }
                }
            }
            {
                let mut cache = config.schema_cache.write().await;
                cache.invalidate_all();
            }
            write_state(&id, "success", "complete", None);
            append_log(&log_file, "repair job complete");
        }
        Ok(st) => {
            let msg = format!("nix-store repair exited with status {st}");
            append_log(&log_file, &msg);
            write_state(&id, "failed", "nix-store-verify-repair", Some(&msg));
        }
        Err(e) => {
            let msg = format!("wait on nix-store failed: {e}");
            append_log(&log_file, &msg);
            write_state(&id, "failed", "nix-store-verify-repair", Some(&msg));
        }
    }
}
