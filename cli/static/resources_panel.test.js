'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { formatBytes, formatRate, formatUptime, pctTone, sortProcs, usedPct } = require('./resources_panel.js');

test('formatBytes: picks the largest unit under 1024 and rounds sensibly', () => {
  assert.equal(formatBytes(0), '0 B');
  assert.equal(formatBytes(512), '512 B');
  assert.equal(formatBytes(1536), '1.5 KB');
  assert.equal(formatBytes(1024 * 1024 * 3), '3 MB');
  assert.equal(formatBytes(1024 * 1024 * 1024 * 2.5), '2.5 GB');
});

test('formatBytes: negative / NaN input clamps to 0 B, never throws', () => {
  assert.equal(formatBytes(-5), '0 B');
  assert.equal(formatBytes(NaN), '0 B');
  assert.equal(formatBytes(undefined), '0 B');
});

test('formatRate: appends /s to the byte formatting', () => {
  assert.equal(formatRate(1024 * 1024), '1 MB/s');
});

test('formatUptime: coarsest two units, drops smaller ones once days show', () => {
  assert.equal(formatUptime(59), '0m');
  assert.equal(formatUptime(125), '2m');
  assert.equal(formatUptime(3600 * 2 + 60 * 5), '2h 5m');
  assert.equal(formatUptime(86400 * 3 + 3600 * 4), '3d 4h');
});

test('formatUptime: negative/garbage clamps to 0m', () => {
  assert.equal(formatUptime(-100), '0m');
  assert.equal(formatUptime(NaN), '0m');
});

test('pctTone: thresholds at 70 and 90', () => {
  assert.equal(pctTone(0), 'success');
  assert.equal(pctTone(69.9), 'success');
  assert.equal(pctTone(70), 'warning');
  assert.equal(pctTone(89.9), 'warning');
  assert.equal(pctTone(90), 'error');
  assert.equal(pctTone(150), 'error');
});

test('usedPct: guards against zero/missing total', () => {
  assert.equal(usedPct(50, 100), 50);
  assert.equal(usedPct(50, 0), 0);
  assert.equal(usedPct(50, undefined), 0);
});

test('sortProcs: sorts numeric fields desc by default, never mutates input', () => {
  const procs = [
    { pid: 1, name: 'b', cpu_pct: 5, mem_bytes: 100 },
    { pid: 2, name: 'a', cpu_pct: 50, mem_bytes: 10 },
    { pid: 3, name: 'c', cpu_pct: 20, mem_bytes: 500 },
  ];
  const byCpuDesc = sortProcs(procs, 'cpu_pct', 'desc');
  assert.deepEqual(byCpuDesc.map((p) => p.pid), [2, 3, 1]);
  // Original array order untouched.
  assert.deepEqual(procs.map((p) => p.pid), [1, 2, 3]);

  const byMemAsc = sortProcs(procs, 'mem_bytes', 'asc');
  assert.deepEqual(byMemAsc.map((p) => p.pid), [2, 1, 3]);
});

test('sortProcs: string fields sort lexicographically', () => {
  const procs = [{ name: 'zeta' }, { name: 'alpha' }, { name: 'mid' }];
  const sorted = sortProcs(procs, 'name', 'asc');
  assert.deepEqual(sorted.map((p) => p.name), ['alpha', 'mid', 'zeta']);
});

test('sortProcs: empty/undefined input is an empty array', () => {
  assert.deepEqual(sortProcs(undefined, 'cpu_pct', 'desc'), []);
  assert.deepEqual(sortProcs([], 'cpu_pct', 'desc'), []);
});
