import test from 'node:test'
import assert from 'node:assert/strict'

test('high frequency noise events are defined for filtering', () => {
  const HIGH_FREQUENCY_IGNORED_EVENTS = new Set([
    'Network.dataReceived',
    'Network.resourceChangedPriority',
    'DOM.childNodeCountUpdated',
    'DOM.attributeModified',
    'DOM.characterDataModified',
  ])

  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Network.dataReceived'), true)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Network.resourceChangedPriority'), true)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('DOM.childNodeCountUpdated'), true)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('DOM.attributeModified'), true)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('DOM.characterDataModified'), true)

  // Essential events must never be filtered
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Target.attachedToTarget'), false)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Target.detachedFromTarget'), false)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Page.loadEventFired'), false)
  assert.equal(HIGH_FREQUENCY_IGNORED_EVENTS.has('Runtime.consoleAPICalled'), false)
})

test('batch commands dispatch sequentially', async () => {
  const dispatched = []
  async function mockDispatch(tabId, method, params) {
    dispatched.push({ tabId, method, params })
    return { status: 'ok', method }
  }

  const commands = [
    { method: 'Input.dispatchMouseEvent', params: { type: 'mousePressed', x: 10, y: 20 } },
    { method: 'Input.insertText', params: { text: 'hello' } },
    { method: 'Input.dispatchKeyEvent', params: { type: 'keyDown', key: 'Enter' } },
  ]

  const results = []
  for (const cmd of commands) {
    results.push(await mockDispatch(1, cmd.method, cmd.params))
  }

  assert.equal(results.length, 3)
  assert.equal(dispatched[0].method, 'Input.dispatchMouseEvent')
  assert.equal(dispatched[1].method, 'Input.insertText')
  assert.equal(dispatched[2].method, 'Input.dispatchKeyEvent')
})
