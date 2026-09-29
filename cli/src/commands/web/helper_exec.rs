//! Run declared option helper bash scripts with a fixed I/O protocol.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

const HELPER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STDOUT: usize = 64 * 1024;
const MAX_STDERR: usize = 16 * 1024;

pub struct HelperExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Invoke `bash script_path` with JSON stdin; capture stdout/stderr with caps.
pub async fn run_helper_script(
    script: &Path,
    stdin_json: &str,
    env_extra: &[(&str, &str)],
) -> Result<HelperExecResult> {
    run_script(script, stdin_json, env_extra, None, HELPER_TIMEOUT).await
}

/// Run a schema-declared script. With `run_as` (widget OAuth helpers) it runs via
/// `sudo -n -u <run_as>` with the caller's env; otherwise under bash with a scrubbed
/// env and a throwaway HOME.
pub async fn run_script(
    script: &Path,
    stdin_json: &str,
    env_extra: &[(&str, &str)],
    run_as: Option<&str>,
    timeout_dur: Duration,
) -> Result<HelperExecResult> {
    if !script.is_absolute() {
        bail!("helper script path must be absolute");
    }
    // Defense-in-depth: release builds only run store-resident helpers.
    // Primary trust is that `script` came from server-side schema, never the client.
    // Debug builds still allow absolute non-store paths for local dev.
    let script_s = script.to_string_lossy();
    if !cfg!(debug_assertions) && !script_s.starts_with("/nix/store/") {
        bail!(
            "helper script must be under /nix/store/ in release builds (got {})",
            script_s
        );
    }

    let tmp = tempfile_dir()?;
    let path = helper_path();

    let mut cmd = if let Some(user) = run_as {
        let sudo = crate::commands::web::util::sudo_cmd();
        let argv = widget_sudo_argv(&sudo, user, &script_s, &path, env_extra);
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        c
    } else {
        let bash = std::env::var("NEO_HELPER_BASH").unwrap_or_else(|_| "bash".to_string());
        let home = tmp.join("home");
        std::fs::create_dir_all(&home).ok();
        let mut c = Command::new(&bash);
        c.arg(script)
            .current_dir(&tmp)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("TMPDIR", &tmp)
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8");
        for (k, v) in env_extra {
            c.env(k, v);
        }
        c
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawn helper bash")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_json.as_bytes())
            .await
            .context("write helper stdin")?;
        drop(stdin);
    }

    let run = async {
        let stdout = read_capped(child.stdout.take(), MAX_STDOUT).await?;
        let stderr = read_capped(child.stderr.take(), MAX_STDERR).await?;
        let status = child.wait().await?;
        Ok::<_, anyhow::Error>((status, stdout, stderr))
    };

    let outcome = timeout(timeout_dur, run).await;
    let _ = std::fs::remove_dir_all(&tmp);
    match outcome {
        Ok(Ok((status, stdout, stderr))) => Ok(HelperExecResult {
            exit_code: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            timed_out: false,
        }),
        Ok(Err(e)) => {
            let _ = child.kill().await;
            Err(e)
        }
        Err(_) => {
            let _ = child.kill().await;
            Ok(HelperExecResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
            })
        }
    }
}

/// Read `pipe` to EOF, keeping at most `max` bytes.
async fn read_capped(
    pipe: Option<impl tokio::io::AsyncRead + Unpin>,
    max: usize,
) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let Some(mut pipe) = pipe else {
        return Ok(out);
    };
    let mut buf = vec![0u8; 4096];
    loop {
        let n = pipe.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if out.len() + n > max {
            out.extend_from_slice(&buf[..n.min(max.saturating_sub(out.len()))]);
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let base = std::env::temp_dir().join("neo-helper");
    std::fs::create_dir_all(&base).ok();
    let dir = base.join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).context("create helper temp dir")?;
    Ok(dir)
}

/// `sudo -n -u <user> VAR=value ... -- <script>`
///
/// The sudoers rule is NOPASSWD:SETENV on `<script>` (e.g. neo-hermes-auth).
/// Wrapping with `env` makes sudo match `/bin/env` instead, which is not
/// allowed — that is what produces `sudo: a password is required`.
fn widget_sudo_argv(
    sudo: &str,
    user: &str,
    script: &str,
    path: &str,
    env_extra: &[(&str, &str)],
) -> Vec<String> {
    let mut argv = vec![
        sudo.to_string(),
        "-n".into(),
        "-u".into(),
        user.to_string(),
        format!("PATH={path}"),
        "LANG=C.UTF-8".into(),
        "LC_ALL=C.UTF-8".into(),
    ];
    for (k, v) in env_extra {
        argv.push(format!("{k}={v}"));
    }
    argv.push("--".into());
    argv.push(script.to_string());
    argv
}

fn helper_path() -> String {
    // Prefer NEO_HELPER_PATH (explicit tool dirs from neo-web), then process PATH,
    // then common NixOS system paths so local `neo web` and activated units both work.
    let mut parts: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("NEO_HELPER_PATH") {
        if !p.is_empty() {
            parts.push(p);
        }
    }
    if let Ok(p) = std::env::var("PATH") {
        if !p.is_empty() {
            parts.push(p);
        }
    }
    parts.push("/run/current-system/sw/bin".into());
    parts.push("/usr/bin".into());
    parts.push("/bin".into());
    parts.join(":")
}

/// Parse helper stdout into a JSON value for form fill.
pub fn parse_helper_value(stdout: &str) -> Result<serde_json::Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        bail!("helper produced empty output");
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(obj) = v.as_object() {
            if let Some(val) = obj.get("value") {
                return Ok(val.clone());
            }
        }
        // bare JSON string/number/bool/array
        return Ok(v);
    }
    // bare string line
    Ok(serde_json::Value::String(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::widget_sudo_argv;

    fn sample_argv() -> Vec<String> {
        widget_sudo_argv(
            "sudo",
            "hermes",
            "/nix/store/abc-neo-hermes-auth/bin/neo-hermes-auth",
            "/run/current-system/sw/bin",
            &[("HERMES_HOME", "/var/neo/DATA/AppData/hermes/.hermes")],
        )
    }

    #[test]
    fn widget_sudo_runs_script_directly_not_via_env() {
        let argv = sample_argv();
        assert!(
            !argv.iter().any(|a| a == "env" || a.ends_with("/env")),
            "sudo must not wrap the helper in env (sudoers matches the binary, not env): {argv:?}"
        );
        assert_eq!(
            argv.last().map(String::as_str),
            Some("/nix/store/abc-neo-hermes-auth/bin/neo-hermes-auth")
        );
    }

    #[test]
    fn widget_sudo_passes_env_via_setenv_assignments_before_command() {
        let argv = sample_argv();
        let dash = argv
            .iter()
            .position(|a| a == "--")
            .expect("sudo argv should end options with --");
        let before: Vec<&str> = argv[..dash].iter().map(String::as_str).collect();
        let after: Vec<&str> = argv[dash + 1..].iter().map(String::as_str).collect();
        assert_eq!(&before[..4], ["sudo", "-n", "-u", "hermes"]);
        assert!(
            before.iter().any(|a| a.starts_with("PATH=")),
            "PATH= must be a sudo SETENV assignment, not an env(1) argument: {argv:?}"
        );
        assert!(before.contains(&"LANG=C.UTF-8"));
        assert!(before.contains(&"LC_ALL=C.UTF-8"));
        assert!(before.contains(&"HERMES_HOME=/var/neo/DATA/AppData/hermes/.hermes"));
        assert_eq!(
            after,
            ["/nix/store/abc-neo-hermes-auth/bin/neo-hermes-auth"]
        );
    }
}
