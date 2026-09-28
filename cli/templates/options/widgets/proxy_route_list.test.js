'use strict';

require('./registry.js');
const widget = require('./proxy_route_list.js');
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { makeForm } = require('./test/harness.js');

const OPT = 'proxyPass';

function routesOption(current, uiExtra) {
  return {
    name: OPT,
    type: { kind: 'attrsOf', elem: { kind: 'str' } },
    default: {},
    current: current !== undefined ? current : {},
    ui: Object.assign({ widget: 'proxyRouteList' }, uiExtra || {}),
  };
}

function formWith(current, uiExtra) {
  const form = makeForm({ options: [routesOption(current, uiExtra)] });
  form.initWidgets();
  return form;
}

/** Add a row with the given domain/upstream and return its id. */
function addRoute(form, domain, upstream) {
  const id = form.prlAdd(OPT);
  form.prlSet(OPT, id, 'domain', domain);
  form.prlSet(OPT, id, 'upstream', upstream);
  return id;
}

function levels(form, id, field) {
  return form.prlIssues(OPT, id)
    .filter((i) => !field || i.field === field)
    .map((i) => i.level);
}

function texts(form, id) {
  return form.prlIssues(OPT, id).map((i) => i.text).join('\n');
}

// ── load / commit ────────────────────────────────────────────────────────────

test('init builds one row per map entry in key order', () => {
  const form = formWith({
    'a.example.com': 'http://10.0.0.1:81',
    'b.example.com': 'http://10.0.0.2:82',
  });
  const rows = form.prlRows(OPT);
  assert.deepEqual(rows.map((r) => [r.domain, r.upstream]), [
    ['a.example.com', 'http://10.0.0.1:81'],
    ['b.example.com', 'http://10.0.0.2:82'],
  ]);
  assert.notEqual(rows[0].id, rows[1].id);
  assert.equal(form.isAtOriginal(OPT), true);
});

test('init tolerates a null / non-object current', () => {
  const form = makeForm({ options: [routesOption(null)] });
  form.values[OPT] = null;
  form.initWidgets();
  assert.deepEqual(form.prlRows(OPT), []);
  assert.deepEqual(form.values[OPT], {});
});

test('a freshly added blank row does not dirty the option', () => {
  const form = formWith({});
  form.prlAdd(OPT);
  assert.equal(form.prlRows(OPT).length, 1);
  assert.deepEqual(form.values[OPT], {});
  assert.equal(form.isAtOriginal(OPT), true);
  assert.equal(form.prlRowStatus(OPT, form.prlRows(OPT)[0].id), '');
  assert.deepEqual(form.widgetValidationErrors(), []);
});

test('editing a row commits trimmed domain → upstream into values', () => {
  const form = formWith({});
  addRoute(form, '  octo.example.com ', ' http://192.168.178.42:8123 ');
  assert.deepEqual(form.values[OPT], { 'octo.example.com': 'http://192.168.178.42:8123' });
  assert.equal(form.isAtOriginal(OPT), false);
});

test('renaming a domain keeps the row (id) and replaces the key', () => {
  const form = formWith({ 'old.example.com': 'http://10.0.0.5:80' });
  const id = form.prlRows(OPT)[0].id;
  form.prlSet(OPT, id, 'domain', 'new.example.com');
  assert.equal(form.prlRows(OPT)[0].id, id);
  assert.deepEqual(form.values[OPT], { 'new.example.com': 'http://10.0.0.5:80' });
});

test('prlSet ignores unknown fields', () => {
  const form = formWith({ 'a.example.com': 'http://10.0.0.5:80' });
  const id = form.prlRows(OPT)[0].id;
  form.prlSet(OPT, id, 'id', 99);
  assert.equal(form.prlRows(OPT)[0].id, id);
});

test('prlRemove drops the row and its key', () => {
  const form = formWith({
    'a.example.com': 'http://10.0.0.1:81',
    'b.example.com': 'http://10.0.0.2:82',
  });
  form.prlRemove(OPT, form.prlRows(OPT)[0].id);
  assert.deepEqual(form.values[OPT], { 'b.example.com': 'http://10.0.0.2:82' });
});

test('row ids stay unique after remove + add', () => {
  const form = formWith({ 'a.example.com': 'http://10.0.0.1:81' });
  const first = form.prlRows(OPT)[0].id;
  form.prlRemove(OPT, first);
  const next = form.prlAdd(OPT);
  assert.notEqual(next, first);
});

// ── domain validation ────────────────────────────────────────────────────────

test('valid route has no issues and status ok', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'http://192.168.178.42:8123');
  assert.deepEqual(form.prlIssues(OPT, id), []);
  assert.equal(form.prlRowStatus(OPT, id), 'ok');
  assert.equal(form.prlRowStatusLabel(OPT, id), 'Ready');
});

test('domain with scheme is an error with a strip fix', () => {
  const form = formWith({});
  const id = addRoute(form, 'https://Octo.example.com/', 'http://10.0.0.2:80');
  const [iss] = form.prlIssues(OPT, id);
  assert.equal(iss.level, 'error');
  assert.equal(iss.field, 'domain');
  assert.deepEqual({ field: iss.fix.field, value: iss.fix.value }, { field: 'domain', value: 'octo.example.com' });
  form.prlApplyFix(OPT, id, iss.fix);
  assert.deepEqual(form.prlIssues(OPT, id), []);
  assert.deepEqual(form.values[OPT], { 'octo.example.com': 'http://10.0.0.2:80' });
});

test('domain with a path or port is an error', () => {
  const form = formWith({});
  const a = addRoute(form, 'a.example.com/app', 'http://10.0.0.2:80');
  const b = addRoute(form, 'b.example.com:8443', 'http://10.0.0.3:80');
  assert.deepEqual(levels(form, a, 'domain'), ['error']);
  assert.match(texts(form, a), /paths are not allowed/);
  assert.deepEqual(levels(form, b, 'domain'), ['error']);
  assert.match(texts(form, b), /443/);
  assert.equal(form.prlIssues(OPT, b)[0].fix.value, 'b.example.com');
});

test('single-label, wildcard, spaces and bad labels are errors', () => {
  const form = formWith({});
  for (const d of ['octo', '*.example.com', 'a b.example.com', '-x.example.com', 'a..example.com', '10.0.0.1']) {
    const id = addRoute(form, d, 'http://10.0.0.2:80');
    assert.deepEqual(levels(form, id, 'domain'), ['error'], `expected error for ${d}`);
  }
});

test('duplicate domains (case-insensitive): later rows error, first wins in values', () => {
  const form = formWith({});
  const a = addRoute(form, 'octo.example.com', 'http://10.0.0.2:80');
  const b = addRoute(form, 'OCTO.example.com', 'http://10.0.0.3:80');
  assert.deepEqual(levels(form, a), []);
  assert.ok(levels(form, b, 'domain').includes('error'));
  assert.match(texts(form, b), /Duplicate domain/);
  assert.deepEqual(form.values[OPT], { 'octo.example.com': 'http://10.0.0.2:80' });
  assert.equal(form.prlCount(OPT, 'error'), 1);
});

test('uppercase domain is a warning with a lowercase fix', () => {
  const form = formWith({});
  const id = addRoute(form, 'Octo.Example.com', 'http://10.0.0.2:80');
  const [iss] = form.prlIssues(OPT, id);
  assert.equal(iss.level, 'warning');
  assert.equal(iss.fix.value, 'octo.example.com');
  assert.equal(form.prlRowStatus(OPT, id), 'warning');
  assert.deepEqual(form.widgetValidationErrors(), []);
});

test('upstream without a domain asks for the domain', () => {
  const form = formWith({});
  const id = addRoute(form, '', 'http://10.0.0.2:80');
  assert.deepEqual(levels(form, id, 'domain'), ['error']);
  assert.deepEqual(form.values[OPT], {});
  assert.equal(form.widgetValidationErrors().length, 1);
  assert.match(form.widgetValidationErrors()[0], /^route 1: /);
});

// ── upstream validation ──────────────────────────────────────────────────────

test('domain without upstream asks for the upstream', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', '');
  assert.deepEqual(levels(form, id, 'upstream'), ['error']);
  assert.deepEqual(form.values[OPT], { 'octo.example.com': '' });
  assert.match(form.widgetValidationErrors()[0], /^octo\.example\.com: /);
});

test('missing scheme is an error with an "Add http://" fix', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', '192.168.1.20:8123');
  const [iss] = form.prlIssues(OPT, id);
  assert.equal(iss.level, 'error');
  assert.equal(iss.fix.value, 'http://192.168.1.20:8123');
  form.prlApplyFix(OPT, id, iss.fix);
  assert.deepEqual(form.prlIssues(OPT, id), []);
});

test('non-http schemes are errors', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'ftp://10.0.0.2:21');
  assert.deepEqual(levels(form, id, 'upstream'), ['error']);
});

test('https upstream warns that it is not needed and offers http', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'https://10.0.0.2:8443');
  const iss = form.prlIssues(OPT, id);
  assert.deepEqual(iss.map((i) => i.level), ['warning']);
  assert.match(iss[0].text, /terminates TLS/);
  assert.equal(iss[0].fix.value, 'http://10.0.0.2:8443');
  assert.deepEqual(form.widgetValidationErrors(), []);
});

test('missing port is an info hint, not a warning', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'http://printer.lan');
  assert.deepEqual(levels(form, id), ['info']);
  assert.match(texts(form, id), /default 80/);
  assert.equal(form.prlRowStatus(OPT, id), 'ok');
});

test('bad ports and hosts are errors', () => {
  const form = formWith({});
  for (const u of ['http://10.0.0.2:0', 'http://10.0.0.2:70000', 'http://10.0.0.2:ab',
    'http://:8080', 'http://300.1.1.1:80', 'http://bad_host-:80', 'http://user@10.0.0.2:80',
    'http://10.0.0.2 :80']) {
    const id = addRoute(form, `r${form.prlRows(OPT).length}.example.com`, u);
    assert.deepEqual(levels(form, id, 'upstream'), ['error'], `expected error for ${u}`);
  }
});

test('IPv6 literal and container-style hostnames are accepted', () => {
  const form = formWith({});
  const a = addRoute(form, 'a.example.com', 'http://[fd00::12]:8080');
  const b = addRoute(form, 'b.example.com', 'http://host.docker.internal:7681');
  const c = addRoute(form, 'c.example.com', 'http://home_assistant:8123');
  assert.deepEqual(levels(form, a), []);
  assert.deepEqual(levels(form, b), []);
  assert.deepEqual(levels(form, c), []);
});

test('path in upstream warns that it replaces the request URI', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'http://10.0.0.2:8123/');
  const iss = form.prlIssues(OPT, id);
  assert.deepEqual(iss.map((i) => i.level), ['warning']);
  assert.match(iss[0].text, /replaces/);
  form.prlApplyFix(OPT, id, iss[0].fix);
  assert.equal(form.values[OPT]['octo.example.com'], 'http://10.0.0.2:8123');
});

test('loopback upstream warns that it points into the proxy container', () => {
  const form = formWith({});
  const id = addRoute(form, 'octo.example.com', 'http://127.0.0.1:8123');
  assert.deepEqual(levels(form, id), ['warning']);
  assert.match(texts(form, id), /host\.docker\.internal/);
});

// ── lifecycle: reset / revert / save ─────────────────────────────────────────

test('revertField restores rows from originals', () => {
  const form = formWith({ 'a.example.com': 'http://10.0.0.1:81' });
  addRoute(form, 'b.example.com', 'http://10.0.0.2:82');
  form.prlRemove(OPT, form.prlRows(OPT)[0].id);
  form.revertField(OPT);
  assert.deepEqual(form.prlRows(OPT).map((r) => r.domain), ['a.example.com']);
  assert.equal(form.isAtOriginal(OPT), true);
});

test('resetField clears rows back to the default map', () => {
  const form = formWith({ 'a.example.com': 'http://10.0.0.1:81' });
  form.resetField(OPT);
  assert.deepEqual(form.prlRows(OPT), []);
  assert.deepEqual(form.values[OPT], {});
});

test('prepareSave omits the option when the map equals the default', () => {
  const form = formWith({});
  form.prlAdd(OPT);
  assert.equal(OPT in form.collectSave(), false);
  addRoute(form, 'octo.example.com', 'http://10.0.0.2:8123');
  assert.deepEqual(form.collectSave()[OPT], { 'octo.example.com': 'http://10.0.0.2:8123' });
});

test('widgetValidationErrors only reports errors, never warnings or info', () => {
  const form = formWith({});
  addRoute(form, 'a.example.com', 'https://10.0.0.2');
  addRoute(form, 'b.example.com', 'nope');
  const errs = form.widgetValidationErrors();
  assert.equal(errs.length, 1);
  assert.match(errs[0], /^b\.example\.com: /);
});

// ── display helpers ──────────────────────────────────────────────────────────

test('summary helpers fall back to placeholders and strip noise', () => {
  const form = formWith({});
  assert.equal(form.prlShownDomain({ domain: '' }), 'your.domain');
  assert.equal(form.prlShownDomain({ domain: 'https://Octo.example.com/x' }), 'octo.example.com');
  assert.equal(form.prlShownUpstream({ upstream: '' }), 'http://host:port');
  assert.equal(form.prlShownUpstream({ upstream: 'http://a:1/' }), 'http://a:1');
});

test('emptyHint / entryLabel come from ui with defaults', () => {
  const bare = formWith({});
  assert.match(bare.prlEmptyHint(OPT), /certificate/);
  assert.equal(bare.prlEntryLabel(OPT), 'Route');
  const custom = formWith({}, { emptyHint: 'Nothing here.', entryLabel: 'Site' });
  assert.equal(custom.prlEmptyHint(OPT), 'Nothing here.');
  assert.equal(custom.prlEntryLabel(OPT), 'Site');
});

test('route count ignores blank rows', () => {
  const form = formWith({ 'a.example.com': 'http://10.0.0.1:81' });
  form.prlAdd(OPT);
  assert.equal(form.prlRouteCount(OPT), 1);
});

test('parseUpstream splits scheme, host, port and path', () => {
  assert.deepEqual(widget.parseUpstream('http://10.0.0.2:8123/x'), {
    scheme: 'http', host: '10.0.0.2', port: '8123', hasPort: true, path: '/x', userinfo: false,
  });
  assert.equal(widget.parseUpstream('10.0.0.2:8123'), null);
  const v6 = widget.parseUpstream('http://[::1]');
  assert.equal(v6.host, '[::1]');
  assert.equal(v6.hasPort, false);
});
