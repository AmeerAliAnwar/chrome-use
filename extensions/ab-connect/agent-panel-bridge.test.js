import test from 'node:test';
import assert from 'node:assert/strict';
import { AgentPanelBridge, isAgentPanelSender } from './agent-panel-bridge.js';

function setup(options = {}) {
  const messages = [], timers = new Map();
  let next = 0;
  const bridge = new AgentPanelBridge({
    send: message => messages.push(message),
    schedule: fn => { timers.set(++next, fn); return next; },
    cancel: id => timers.delete(id), ...options,
  });
  return { bridge, messages, timers };
}

test('out-of-order native replies retain their request identity', async () => {
  const { bridge, messages, timers } = setup();
  const a = bridge.request('discover'), b = bridge.request('discover');
  bridge.receive({ method: 'agentPanelReply', requestId: messages[1].requestId, result: { schemaVersion: 1, agents: [] } });
  bridge.receive({ method: 'agentPanelReply', requestId: messages[0].requestId, error: 'Host discovery failed' });
  assert.deepEqual(await b, { schemaVersion: 1, agents: [] });
  await assert.rejects(a, /Host discovery failed/);
  assert.equal(timers.size, 0);
});

test('disconnect rejects work; stale replies cannot settle a new connection', async () => {
  const { bridge, messages } = setup();
  const old = bridge.request('discover');
  bridge.disconnect('Connection ended');
  await assert.rejects(old, /Connection ended/);
  const current = bridge.request('discover');
  assert.notEqual(messages[0].requestId, messages[1].requestId);
  bridge.receive({ method: 'agentPanelReply', requestId: messages[0].requestId, result: 'stale' });
  assert.equal(bridge.pending.size, 1);
  bridge.receive({ method: 'agentPanelReply', requestId: messages[1].requestId, result: { schemaVersion: 1, agents: [] } });
  assert.deepEqual(await current, { schemaVersion: 1, agents: [] });
});

test('timeout names uncertainty and never resends', async () => {
  const { bridge, messages, timers } = setup();
  const result = bridge.request('discover');
  [...timers.values()][0]();
  await assert.rejects(result, /not replayed/);
  assert.equal(messages.length, 1);
  assert.equal(bridge.pending.size, 0);
  assert.equal(bridge.receive({ method: 'agentPanelReply', requestId: messages[0].requestId, result: 'late' }), true);
});

test('malformed native reply fails instead of claiming success', async () => {
  const { bridge, messages } = setup();
  const result = bridge.request('discover');
  bridge.receive({ method: 'agentPanelReply', requestId: messages[0].requestId });
  await assert.rejects(result, /Malformed/);
});

test('unsupported schema and null results do not report success', async () => {
  for (const result of [null, { schemaVersion: 2, agents: [] }, { schemaVersion: 1, agents: null }]) {
    const { bridge, messages } = setup();
    const pending = bridge.request('discover');
    bridge.receive({ method: 'agentPanelReply', requestId: messages[0].requestId, result });
    await assert.rejects(pending, /Malformed/);
  }
});

test('worker recreation cannot reuse the previous worker request ids', async () => {
  const a = setup(), b = setup();
  const pendingA = a.bridge.request('discover'), pendingB = b.bridge.request('discover');
  assert.notEqual(a.messages[0].requestId, b.messages[0].requestId);
  a.bridge.disconnect(); b.bridge.disconnect();
  await assert.rejects(pendingA, /disconnected/);
  await assert.rejects(pendingB, /disconnected/);
});

test('failed send and disconnected port do not leave a pending request', async () => {
  const { bridge, timers } = setup({ send() { throw new Error('No host'); } });
  await assert.rejects(bridge.request('discover'), /No host/);
  assert.equal(bridge.pending.size, 0);
  assert.equal(timers.size, 0);
});

test('unknown operations and overload fail before reaching the host', async () => {
  const { bridge, messages } = setup({ maxPending: 1 });
  await assert.rejects(bridge.request('arbitrary-shell'), /Unsupported/);
  const pending = bridge.request('discover');
  await assert.rejects(bridge.request('discover'), /Too many/);
  assert.equal(messages.length, 1);
  bridge.disconnect();
  await assert.rejects(pending, /disconnected/);
});

test('non-panel CDP and health messages pass through unchanged', () => {
  const { bridge } = setup();
  assert.equal(bridge.receive({ method: 'pong' }), false);
  assert.equal(bridge.receive({ method: 'forwardCDPCommand', params: {} }), false);
});

test('only packaged extension pages may invoke native agent services', () => {
  const runtime = { id: 'own', getURL: path => `chrome-extension://own/${path}` };
  assert.equal(isAgentPanelSender({ id: 'own', url: runtime.getURL('sidepanel.html') }, runtime), true);
  assert.equal(isAgentPanelSender({ id: 'own', url: runtime.getURL('options.html') }, runtime), true);
  for (const sender of [
    { id: 'other', url: runtime.getURL('sidepanel.html') },
    { id: 'own', url: 'https://example.com/' },
    { id: 'own', url: runtime.getURL('sidepanel.html'), tab: { id: 1 } },
    { id: 'own', url: runtime.getURL('sidepanel.html') + '?spoof' },
  ]) assert.equal(isAgentPanelSender(sender, runtime), false);
});
