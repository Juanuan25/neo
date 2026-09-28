// Service health: one shared summarizer + page-wide status dots.
//
// NeoUnitHealth.summarize(entries) folds per-unit ActiveState into one service
// state. The option pane (configuration.js unitSummary) and the services grid /
// navigator dots both call it, so they always agree.
//
// Dots: any element with data-service-status="<name>" data-units="a b"
// data-timers="t". After render (never blocking it), every dot on the page is
// refreshed with ONE POST /status/services; the server answers from a single
// batched `systemctl show` behind a few-second single-flight cache. Polls every
// 15s while the tab is visible; pauses while hidden.
(function (root, factory) {
  var api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  if (root) {
    root.NeoUnitHealth = api.health;
    root.NeoServiceStatus = api.dots;
    api.dots.autostart(root);
  }
})(typeof window !== 'undefined' ? window : null, function () {
  'use strict';

  var BUSY = { activating: 1, deactivating: 1, reloading: 1, refreshing: 1 };

  /**
   * @param {Array<{state: ?string, timer?: boolean, load?: string}>} entries
   *   state: ActiveState, or null/'' while not known yet (pane waiting on WS).
   *   load:  LoadState when known ('not-found' = unit not deployed yet).
   * @returns {{state: string, text: string}}
   *   state ∈ ok | partial | changing | failed | down | none | unknown
   */
  function summarize(entries) {
    entries = entries || [];
    if (!entries.length) return { state: 'none', text: 'no units' };
    var total = 0, active = 0, failed = 0, busy = 0, unknown = 0, missing = 0, pending = 0;
    for (var i = 0; i < entries.length; i++) {
      var e = entries[i] || {};
      var st = e.state;
      if (!st) { pending++; continue; }
      // Timer-backed oneshots idle between runs; only a failure counts against them.
      if (e.timer && st !== 'failed') continue;
      total++;
      if (e.load === 'not-found') { missing++; continue; }
      if (st === 'active') active++;
      else if (st === 'failed') failed++;
      else if (BUSY[st]) busy++;
      else if (st === 'unknown') unknown++;
    }
    if (pending) return { state: 'unknown', text: 'checking…' };
    if (failed) return { state: 'failed', text: failed === 1 ? '1 failed' : failed + ' failed' };
    if (total && missing === total) return { state: 'none', text: 'not deployed' };
    if (unknown) return { state: 'none', text: 'status unknown' };
    if (busy) return { state: 'changing', text: 'changing…' };
    if (total === 0) return { state: 'ok', text: 'idle' };
    if (active === total) return { state: 'ok', text: 'running' };
    if (active === 0) return { state: 'down', text: 'stopped' };
    return { state: 'partial', text: active + '/' + total + ' running' };
  }

  function words(s) {
    return String(s || '').split(/\s+/).filter(Boolean);
  }

  /** Request body for every dot under `scope` (deduped by service name). */
  function collectRequest(scope) {
    var els = scope.querySelectorAll('[data-service-status]');
    var services = {};
    var n = 0;
    for (var i = 0; i < els.length; i++) {
      var name = els[i].getAttribute('data-service-status');
      if (!name || services[name]) continue;
      services[name] = {
        units: words(els[i].getAttribute('data-units')),
        timers: words(els[i].getAttribute('data-timers')),
      };
      n++;
    }
    return { count: n, body: { services: services } };
  }

  /** Server response row → summarize() input. */
  function entriesFor(resp, name) {
    var svc = resp && resp.services && resp.services[name];
    if (!svc || !svc.units) return null;
    return svc.units.map(function (u) {
      return { state: u.active || 'unknown', timer: !!u.timer, load: u.load };
    });
  }

  function paint(el, summary) {
    var name = el.getAttribute('data-service-status');
    el.setAttribute('data-state', summary.state);
    el.setAttribute('title', name + ': ' + summary.text);
    el.setAttribute('aria-label', name + ': ' + summary.text);
  }

  /** Apply one response to every dot under `scope`. */
  function apply(scope, resp) {
    var els = scope.querySelectorAll('[data-service-status]');
    for (var i = 0; i < els.length; i++) {
      var entries = entriesFor(resp, els[i].getAttribute('data-service-status'));
      paint(els[i], entries ? summarize(entries) : { state: 'none', text: 'status unknown' });
    }
  }

  var POLL_MS = 15000;

  var dots = {
    POLL_MS: POLL_MS,
    collectRequest: collectRequest,
    apply: apply,
    _timer: null,
    _inflight: false,
    _again: false,
    _win: null,

    /** One batched request for all dots on the page (coalesces overlapping calls). */
    refresh: function () {
      var win = dots._win;
      if (!win || !win.document) return;
      var doc = win.document;
      var req = collectRequest(doc);
      if (!req.count) return;
      if (dots._inflight) { dots._again = true; return; }
      dots._inflight = true;
      var done = function () {
        dots._inflight = false;
        if (dots._again) { dots._again = false; dots.refresh(); }
      };
      win.fetch('/status/services', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        credentials: 'same-origin',
        body: JSON.stringify(req.body),
      })
        .then(function (r) { return r.ok ? r.json() : null; })
        .then(function (resp) { apply(doc, resp || {}); })
        .catch(function () { apply(doc, {}); })
        .then(done, done);
    },

    _schedule: function () {
      var win = dots._win;
      if (dots._timer) { win.clearTimeout(dots._timer); dots._timer = null; }
      if (win.document.visibilityState === 'hidden') return;
      if (!win.document.querySelector('[data-service-status]')) return;
      dots._timer = win.setTimeout(function () {
        dots._timer = null;
        dots.refresh();
        dots._schedule();
      }, POLL_MS);
    },

    /** Refresh now and restart the poll clock. */
    kick: function () {
      dots.refresh();
      dots._schedule();
    },

    autostart: function (win) {
      if (!win || !win.document || typeof win.fetch !== 'function') return;
      dots._win = win;
      var doc = win.document;
      var start = function () {
        // Next tick: never compete with first paint.
        win.setTimeout(dots.kick, 0);
      };
      if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', start);
      else start();

      doc.addEventListener('visibilitychange', function () {
        if (doc.visibilityState === 'visible') dots.kick();
        else dots._schedule(); // clears the timer while hidden
      });

      // htmx swaps (grid partial, nav-services) may bring new dots: fetch once for them.
      var pendingSwap = null;
      doc.addEventListener('htmx:afterSettle', function (evt) {
        var t = evt && evt.detail && evt.detail.target;
        if (!t || !t.querySelector) return;
        var fresh = (t.matches && t.matches('[data-service-status]')) ||
          t.querySelector('[data-service-status]:not([data-state])');
        if (!fresh) return;
        if (pendingSwap) win.clearTimeout(pendingSwap);
        pendingSwap = win.setTimeout(function () { pendingSwap = null; dots.kick(); }, 50);
      });
    },
  };

  return { health: { summarize: summarize }, dots: dots };
});
