'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const m = require('./op_monitor.js');

test('backoff grows and caps', () => {
  assert.equal(m.backoff(0), 300);
  assert.ok(m.backoff(2) > m.backoff(1));
  assert.equal(m.backoff(50), m.backoff(4));
});

test('wsUrl picks scheme and carries the resume offset', () => {
  assert.equal(
    m.wsUrl({ protocol: 'https:', host: 'neo.example' }, 'activation_1', 42),
    'wss://neo.example/ws/op/activation_1?offset=42'
  );
  assert.equal(m.wsUrl({ protocol: 'http:', host: 'h:1' }, 'update_x'), 'ws://h:1/ws/op/update_x?offset=0');
});

test('stepStates: running, failed and success', () => {
  assert.deepEqual(m.stepStates(4, 2, 'in_progress'), ['done', 'done', 'current', 'pending']);
  assert.deepEqual(m.stepStates(3, 1, 'failed'), ['done', 'failed', 'pending']);
  assert.deepEqual(m.stepStates(3, 2, 'success'), ['done', 'done', 'done']);
});

test('statusView tones', () => {
  assert.equal(m.statusView('in_progress').tone, 'running');
  assert.equal(m.statusView('success').tone, 'success');
  assert.equal(m.statusView('failed').tone, 'error');
  assert.equal(m.statusView('').label, 'Connecting');
});

test('outcome: activation success offers reload; update does not', () => {
  const a = m.outcome({ kind: 'activation', status: 'success', branch: 'act-1' });
  assert.equal(a.reload, true);
  assert.match(a.text, /act-1/);
  assert.ok(!m.outcome({ kind: 'update', status: 'success' }).reload);
  assert.equal(m.outcome({ kind: 'update', status: 'failed', error: 'boom' }).text, 'boom');
  assert.equal(m.outcome({ status: 'in_progress' }), null);
});

test('humanPhase', () => {
  assert.equal(m.humanPhase('toplevel-build'), 'toplevel build');
});
