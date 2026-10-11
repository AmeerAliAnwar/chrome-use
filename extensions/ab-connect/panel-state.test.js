import assert from 'node:assert/strict';
import test from 'node:test';
import { PanelTimeline } from './panel-state.js';
const event = (sequence, kind, data) => ({ sequence, kind, data, at: sequence });

test('reconnect replay cannot duplicate user messages or partial output', () => {
  const timeline = new PanelTimeline();
  const events = [event(1, 'user', { messageId: 'one', text: 'fixture' }), event(2, 'textDelta', { itemId: 'answer', text: 'partial' })];
  timeline.apply(events, 2); timeline.apply(events, 2);
  assert.equal(timeline.rows.length, 2); assert.equal(timeline.rows[1].text, 'partial');
});
test('completed chunks replace streamed text rather than duplicating the response', () => {
  const timeline = new PanelTimeline();
  timeline.apply([event(1, 'textDelta', { itemId: 'answer', text: 'partial' }), event(2, 'textFinal', { itemId: 'answer', text: 'full ', part: 0 }), event(3, 'textFinal', { itemId: 'answer', text: 'response', part: 1 })], 3);
  assert.equal(timeline.rows.length, 1); assert.equal(timeline.rows[0].text, 'full response');
});
test('history gaps and bad chunk order never look like a complete transcript', () => {
  const timeline = new PanelTimeline();
  assert.throws(() => timeline.apply([event(2, 'user', { text: 'fixture' })], 2), /gap/);
  assert.equal(timeline.cursor, 0);
  assert.throws(() => timeline.apply([event(1, 'textFinal', { itemId: 'answer', text: 'tail', part: 1 })], 1), /order/);
});
test('permission replies remain pending until a backend resolution or terminal event', () => {
  const timeline = new PanelTimeline();
  timeline.apply([event(1, 'request', { requestId: 7, method: 'fixture' }), event(2, 'replySent', { requestId: 7 })], 2);
  assert.equal(timeline.requests.get('7').sent, true);
  timeline.apply([event(3, 'requestResolved', { requestId: 7 })], 3); assert.equal(timeline.requests.size, 0);
});
test('a returned tool is not labeled successful when its outcome is unknown', () => {
  const timeline = new PanelTimeline();
  timeline.apply([event(1, 'tool', { completed: true, item: { id: 'one', name: 'click', output: 'unconfirmed' } })], 1);
  assert.equal(timeline.rows[0].completed, true); assert.equal(timeline.rows[0].failed, false); assert.equal(timeline.rows[0].text, 'unconfirmed');
});

test('a reused backend item id in another turn remains a separate response', () => {
  const timeline = new PanelTimeline();
  timeline.apply([event(1, 'textDelta', { messageId: 'one', itemId: 'same', text: 'first' }), event(2, 'textDelta', { messageId: 'two', itemId: 'same', text: 'second' })], 2);
  assert.equal(timeline.rows.length, 2); assert.equal(timeline.rows[0].text, 'first'); assert.equal(timeline.rows[1].text, 'second');
});
