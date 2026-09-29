//! Batched systemd unit status for many services at once.
//!
//! One `systemctl show --property=Id,LoadState,ActiveState,SubState -- u1 u2 …`
//! process answers every unit on the page; [`UnitStatusCache`] keeps the result for
//! a few seconds and serializes refreshes (single-flight) so concurrent page loads
//! and pollers share one query instead of hitting systemd per unit.
//!
//! The per-service summary (running / failed / …) is computed client-side by
//! `NeoUnitHealth.summarize` (static/service_status.js), the same function the
//! option pane header uses, so the grid dots and the pane badge always agree.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::process::Command as AsyncCommand;
use tokio::sync::Mutex as AsyncMutex;

use super::super::util::{service_name_ok, systemctl_bin, unit_name_valid};

/// Properties requested from systemd (order in the output is systemd's, not ours).
const SHOW_PROPERTIES: &str = "Id,LoadState,ActiveState,SubState";
/// Upper bound on units per batched query (guards the argv and the cache).
pub const MAX_UNITS: usize = 512;
/// Upper bound on services per status request.
pub const MAX_SERVICES: usize = 256;
/// A wedged systemd must not hang the status endpoint (or the cache lock) forever.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Page-level status cache lifetime: short enough to feel live, long enough that
/// many tabs / pollers collapse onto one systemctl call.
pub const STATUS_CACHE_TTL: Duration = Duration::from_secs(3);

const UNIT_SUFFIXES: &[&str] = &[
    "service",
    "target",
    "timer",
    "socket",
    "mount",
    "automount",
    "path",
    "slice",
    "scope",
    "device",
    "swap",
];

/// Load/active/sub state of one systemd unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnitStatus {
    /// ActiveState (`active`, `inactive`, `failed`, `activating`, …) or `unknown`.
    pub active: String,
    /// SubState (`running`, `dead`, `exited`, …); empty when unknown.
    pub sub: String,
    /// LoadState (`loaded`, `not-found`, …); empty when unknown.
    pub load: String,
}

impl UnitStatus {
    pub fn unknown() -> Self {
        Self {
            active: "unknown".into(),
            sub: String::new(),
            load: String::new(),
        }
    }
}

/// `docker-foo` → `docker-foo.service`; names with a unit-type suffix stay as-is.
fn canonical_unit_id(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, suffix)) if UNIT_SUFFIXES.contains(&suffix) => name.to_string(),
        _ => format!("{name}.service"),
    }
}

fn block_status(block: &HashMap<&str, &str>) -> UnitStatus {
    let get = |k: &str| block.get(k).map(|v| v.trim()).unwrap_or("").to_string();
    let active = get("ActiveState");
    UnitStatus {
        active: if active.is_empty() {
            "unknown".into()
        } else {
            active
        },
        sub: get("SubState"),
        load: get("LoadState"),
    }
}

/// Parse `systemctl show` output for `requested` units (in the order they were passed).
///
/// systemd prints one blank-line-separated `Key=Value` block per argument, in argument
/// order. Aliases resolve to the real unit (`dbus` → `Id=dbus-broker.service`), so the
/// primary mapping is positional; if the block count does not line up (partial output),
/// fall back to matching `Id` against the canonical unit name. Anything unmatched is
/// `unknown`.
pub fn parse_show_output(stdout: &str, requested: &[String]) -> HashMap<String, UnitStatus> {
    let mut blocks: Vec<HashMap<&str, &str>> = Vec::new();
    let mut cur: HashMap<&str, &str> = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            cur.insert(k.trim(), v);
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }

    let mut out = HashMap::with_capacity(requested.len());
    if blocks.len() == requested.len() {
        for (name, block) in requested.iter().zip(blocks.iter()) {
            out.insert(name.clone(), block_status(block));
        }
        return out;
    }

    let by_id: HashMap<&str, &HashMap<&str, &str>> = blocks
        .iter()
        .filter_map(|b| b.get("Id").map(|id| (id.trim(), b)))
        .collect();
    for name in requested {
        let status = by_id
            .get(canonical_unit_id(name).as_str())
            .map(|b| block_status(b))
            .unwrap_or_else(UnitStatus::unknown);
        out.insert(name.clone(), status);
    }
    out
}

/// Valid, de-duplicated unit names (first occurrence order), capped at [`MAX_UNITS`].
fn sanitize_units<'a>(units: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    units
        .into_iter()
        .filter(|u| unit_name_valid(u) && !u.starts_with('-'))
        .filter(|u| seen.insert(u.as_str()))
        .take(MAX_UNITS)
        .cloned()
        .collect()
}

fn all_unknown(units: &[String]) -> HashMap<String, UnitStatus> {
    units
        .iter()
        .map(|u| (u.clone(), UnitStatus::unknown()))
        .collect()
}

/// One `systemctl show` for all `units`. Never errors: when systemctl is missing,
/// times out or prints nothing, every unit is `unknown`.
///
/// Read-only property queries need no privileges, so this runs systemctl directly
/// (no sudo → no per-call auth log noise).
pub async fn query_unit_states(units: &[String]) -> HashMap<String, UnitStatus> {
    let units = sanitize_units(units);
    if units.is_empty() {
        return HashMap::new();
    }
    let mut cmd = AsyncCommand::new(systemctl_bin());
    cmd.arg("show")
        .arg("--no-pager")
        .arg(format!("--property={SHOW_PROPERTIES}"))
        .arg("--")
        .args(&units)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(QUERY_TIMEOUT, cmd.output()).await {
        Ok(Ok(o)) if !o.stdout.is_empty() => {
            parse_show_output(&String::from_utf8_lossy(&o.stdout), &units)
        }
        _ => all_unknown(&units),
    }
}

/// Short-lived, single-flight cache of unit states shared by all status requests.
///
/// The async mutex is held across the refresh, so concurrent callers queue behind
/// the in-flight query and then read its fresh result instead of spawning their own.
#[derive(Debug)]
pub struct UnitStatusCache {
    ttl: Duration,
    entries: AsyncMutex<HashMap<String, (Instant, UnitStatus)>>,
}

impl Default for UnitStatusCache {
    fn default() -> Self {
        Self::new(STATUS_CACHE_TTL)
    }
}

impl UnitStatusCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: AsyncMutex::new(HashMap::new()),
        }
    }

    /// States for `units`, refreshing stale/missing ones with one batched systemctl call.
    pub async fn get(&self, units: &[String]) -> HashMap<String, UnitStatus> {
        self.get_with(
            units,
            |missing| async move { query_unit_states(&missing).await },
        )
        .await
    }

    /// [`Self::get`] with an injectable batch fetcher (tests).
    pub async fn get_with<F, Fut>(&self, units: &[String], fetch: F) -> HashMap<String, UnitStatus>
    where
        F: FnOnce(Vec<String>) -> Fut,
        Fut: Future<Output = HashMap<String, UnitStatus>>,
    {
        let units = sanitize_units(units);
        if units.is_empty() {
            return HashMap::new();
        }
        let mut entries = self.entries.lock().await;
        let now = Instant::now();
        let stale: Vec<String> = units
            .iter()
            .filter(|u| {
                entries
                    .get(u.as_str())
                    .is_none_or(|(at, _)| now.duration_since(*at) >= self.ttl)
            })
            .cloned()
            .collect();
        if !stale.is_empty() {
            let fetched = fetch(stale.clone()).await;
            let at = Instant::now();
            for u in stale {
                let st = fetched.get(&u).cloned().unwrap_or_else(UnitStatus::unknown);
                entries.insert(u, (at, st));
            }
            // Bound memory: drop entries nobody asked about for a while.
            let keep = self.ttl * 20;
            entries.retain(|_, (t, _)| at.duration_since(*t) < keep);
        }
        units
            .into_iter()
            .map(|u| {
                let st = entries
                    .get(&u)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_else(UnitStatus::unknown);
                (u, st)
            })
            .collect()
    }
}

/// Units + timer-backed units the page wants for one service.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct ServiceUnitsRequest {
    #[serde(default)]
    pub units: Vec<String>,
    #[serde(default)]
    pub timers: Vec<String>,
}

/// Body of `POST /status/services`: every service with a dot on the page.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct ServiceStatusRequest {
    #[serde(default)]
    pub services: BTreeMap<String, ServiceUnitsRequest>,
}

/// One unit row in the response (input shape for `NeoUnitHealth.summarize`).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct UnitStatusEntry {
    pub name: String,
    pub active: String,
    pub sub: String,
    pub load: String,
    pub timer: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    pub units: Vec<UnitStatusEntry>,
}

#[derive(Serialize, Debug, Default)]
pub struct ServiceStatusResponse {
    pub services: BTreeMap<String, ServiceStatus>,
}

impl ServiceStatusRequest {
    /// Valid services (capped), each with its valid units.
    fn services(&self) -> impl Iterator<Item = (&String, &ServiceUnitsRequest)> {
        self.services
            .iter()
            .filter(|(name, _)| service_name_ok(name))
            .take(MAX_SERVICES)
    }

    /// De-duplicated union of every requested unit → the argv of the one batched query.
    pub fn all_units(&self) -> Vec<String> {
        sanitize_units(self.services().flat_map(|(_, s)| s.units.iter()))
    }
}

/// Group batched unit states back onto the services that asked for them.
pub fn map_services(
    req: &ServiceStatusRequest,
    states: &HashMap<String, UnitStatus>,
) -> ServiceStatusResponse {
    let mut services = BTreeMap::new();
    for (name, svc) in req.services() {
        let units = sanitize_units(svc.units.iter())
            .into_iter()
            .map(|u| {
                let st = states.get(&u).cloned().unwrap_or_else(UnitStatus::unknown);
                UnitStatusEntry {
                    timer: svc.timers.contains(&u),
                    name: u,
                    active: st.active,
                    sub: st.sub,
                    load: st.load,
                }
            })
            .collect();
        services.insert(name.clone(), ServiceStatus { units });
    }
    ServiceStatusResponse { services }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn st(active: &str, sub: &str, load: &str) -> UnitStatus {
        UnitStatus {
            active: active.into(),
            sub: sub.into(),
            load: load.into(),
        }
    }

    const SAMPLE: &str = "Id=dbus-broker.service\nLoadState=loaded\nActiveState=active\nSubState=running\n\nId=docker-immich.service\nLoadState=loaded\nActiveState=failed\nSubState=failed\n\nId=docker-gone.service\nLoadState=not-found\nActiveState=inactive\nSubState=dead\n";

    #[test]
    fn parse_positional_handles_aliases() {
        let req = s(&["dbus", "docker-immich", "docker-gone"]);
        let m = parse_show_output(SAMPLE, &req);
        assert_eq!(m["dbus"], st("active", "running", "loaded"));
        assert_eq!(m["docker-immich"], st("failed", "failed", "loaded"));
        assert_eq!(m["docker-gone"], st("inactive", "dead", "not-found"));
    }

    #[test]
    fn parse_property_order_does_not_matter() {
        let out = "ActiveState=activating\nSubState=start\nId=neo-x.target\nLoadState=loaded\n";
        let m = parse_show_output(out, &s(&["neo-x.target"]));
        assert_eq!(m["neo-x.target"], st("activating", "start", "loaded"));
    }

    #[test]
    fn parse_count_mismatch_falls_back_to_id_match() {
        // Only two blocks for three units: match by canonical Id, rest unknown.
        let out = "Id=docker-a.service\nActiveState=active\nSubState=running\nLoadState=loaded\n\n\nId=b.timer\nActiveState=active\nSubState=waiting\nLoadState=loaded\n";
        let m = parse_show_output(out, &s(&["docker-a", "b.timer", "c"]));
        assert_eq!(m["docker-a"].active, "active");
        assert_eq!(m["b.timer"].sub, "waiting");
        assert_eq!(m["c"], UnitStatus::unknown());
    }

    #[test]
    fn parse_empty_or_garbage_is_unknown() {
        let m = parse_show_output("", &s(&["a", "b"]));
        assert_eq!(m["a"], UnitStatus::unknown());
        assert_eq!(m["b"], UnitStatus::unknown());
        let m = parse_show_output("Id=a.service\nLoadState=loaded\n", &s(&["a"]));
        assert_eq!(m["a"].active, "unknown");
        assert_eq!(m["a"].load, "loaded");
    }

    #[test]
    fn parse_tolerates_crlf_and_extra_blank_lines() {
        let out = "\r\nId=a.service\r\nActiveState=active\r\nSubState=running\r\nLoadState=loaded\r\n\r\n\r\n";
        let m = parse_show_output(out, &s(&["a"]));
        assert_eq!(m["a"], st("active", "running", "loaded"));
    }

    #[test]
    fn canonical_ids() {
        assert_eq!(canonical_unit_id("docker-foo"), "docker-foo.service");
        assert_eq!(canonical_unit_id("neo-foo.target"), "neo-foo.target");
        assert_eq!(canonical_unit_id("app.v2"), "app.v2.service");
    }

    fn request() -> ServiceStatusRequest {
        serde_json::from_str(
            r#"{"services":{
                "immich":{"units":["docker-immich","docker-immich-db","docker-immich"]},
                "pihole":{"units":["docker-pihole","pihole-update-gravity"],"timers":["pihole-update-gravity"]},
                "bad name":{"units":["x"]},
                "evil":{"units":["-H","ok; rm","docker-evil"]},
                "empty":{}
            }}"#,
        )
        .unwrap()
    }

    #[test]
    fn all_units_is_deduped_valid_union() {
        let units = request().all_units();
        assert_eq!(
            units,
            s(&[
                "docker-evil",
                "docker-immich",
                "docker-immich-db",
                "docker-pihole",
                "pihole-update-gravity"
            ])
        );
    }

    #[test]
    fn map_services_groups_states_and_flags_timers() {
        let req = request();
        let units = req.all_units();
        let out = "Id=docker-evil.service\nLoadState=loaded\nActiveState=active\nSubState=running\n\n\
                   Id=docker-immich.service\nLoadState=loaded\nActiveState=active\nSubState=running\n\n\
                   Id=docker-immich-db.service\nLoadState=loaded\nActiveState=failed\nSubState=failed\n\n\
                   Id=docker-pihole.service\nLoadState=loaded\nActiveState=activating\nSubState=start\n\n\
                   Id=pihole-update-gravity.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\n";
        let states = parse_show_output(out, &units);
        let resp = map_services(&req, &states);

        assert!(!resp.services.contains_key("bad name"));
        assert!(resp.services["empty"].units.is_empty());

        let immich = &resp.services["immich"].units;
        assert_eq!(immich.len(), 2, "duplicate unit collapsed");
        assert_eq!(immich[0].name, "docker-immich");
        assert_eq!(immich[0].active, "active");
        assert_eq!(immich[1].active, "failed");
        assert!(!immich[0].timer);

        let pihole = &resp.services["pihole"].units;
        assert_eq!(pihole[0].active, "activating");
        assert!(!pihole[0].timer);
        assert_eq!(pihole[1].name, "pihole-update-gravity");
        assert!(pihole[1].timer);

        let evil = &resp.services["evil"].units;
        assert_eq!(evil.len(), 1);
        assert_eq!(evil[0].name, "docker-evil");
    }

    #[test]
    fn map_services_missing_state_is_unknown() {
        let resp = map_services(&request(), &HashMap::new());
        assert!(resp.services["immich"]
            .units
            .iter()
            .all(|u| u.active == "unknown"));
    }

    fn fake_fetch(
        calls: Arc<AtomicUsize>,
        seen: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    ) -> impl FnOnce(
        Vec<String>,
    )
        -> std::pin::Pin<Box<dyn Future<Output = HashMap<String, UnitStatus>> + Send>> {
        move |units: Vec<String>| {
            calls.fetch_add(1, Ordering::SeqCst);
            seen.lock().unwrap().push(units.clone());
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                units
                    .into_iter()
                    .map(|u| (u, st("active", "running", "loaded")))
                    .collect()
            })
        }
    }

    #[tokio::test]
    async fn cache_reuses_fresh_entries_and_fetches_only_missing() {
        let cache = UnitStatusCache::new(Duration::from_secs(60));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));

        let m = cache
            .get_with(&s(&["a", "b"]), fake_fetch(calls.clone(), seen.clone()))
            .await;
        assert_eq!(m.len(), 2);
        cache
            .get_with(&s(&["a", "b"]), fake_fetch(calls.clone(), seen.clone()))
            .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "fresh hit: no second query"
        );

        cache
            .get_with(&s(&["a", "c"]), fake_fetch(calls.clone(), seen.clone()))
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(seen.lock().unwrap()[1], s(&["c"]), "only the missing unit");
    }

    #[tokio::test]
    async fn cache_expires_after_ttl() {
        let cache = UnitStatusCache::new(Duration::from_millis(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        for _ in 0..3 {
            cache
                .get_with(&s(&["a"]), fake_fetch(calls.clone(), seen.clone()))
                .await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn cache_is_single_flight_under_concurrency() {
        let cache = Arc::new(UnitStatusCache::new(Duration::from_secs(60)));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let cache = cache.clone();
            let f = fake_fetch(calls.clone(), seen.clone());
            handles.push(tokio::spawn(async move {
                cache.get_with(&s(&["a", "b", "c"]), f).await
            }));
        }
        for h in handles {
            assert_eq!(h.await.unwrap().len(), 3);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one batched query total");
    }

    /// Real systemctl when present (states vary by host); without it every unit is
    /// `unknown`. Either way: one entry per requested unit, no error.
    #[tokio::test]
    async fn query_unit_states_always_answers_every_unit() {
        let units = s(&["neo-status-test-nonexistent", "dbus", "dbus"]);
        let m = query_unit_states(&units).await;
        assert_eq!(m.len(), 2);
        let missing = &m["neo-status-test-nonexistent"];
        assert!(
            missing.active == "unknown" || missing.load == "not-found",
            "{missing:?}"
        );
    }

    #[tokio::test]
    async fn cache_ignores_invalid_units_and_empty_requests() {
        let cache = UnitStatusCache::new(Duration::from_secs(60));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let m = cache
            .get_with(
                &s(&["", "-x", "a b"]),
                fake_fetch(calls.clone(), seen.clone()),
            )
            .await;
        assert!(m.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
