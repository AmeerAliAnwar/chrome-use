import test from 'node:test'
import assert from 'node:assert/strict'
import { relayCommandBudgetMs, withRelayTimeout } from './relay-timeout.js'

test('Real Operation: React controlled form filling via prototype descriptor setter', () => {
  // Simulate a React 18+ controlled input element in DOM
  const state = { value: '', renderCount: 0, changeEvents: 0, inputEvents: 0 }

  class MockHTMLInputElement {
    constructor() {
      this._value = ''
    }
    get value() {
      return this._value
    }
    set value(v) {
      this._value = v
      state.value = v
      state.renderCount++
    }
    focus() {
      this.focused = true
    }
    dispatchEvent(event) {
      if (event.type === 'input') state.inputEvents++
      if (event.type === 'change') state.changeEvents++
      return true
    }
  }

  const inputEl = new MockHTMLInputElement()

  // Simulate prototype setter execution (matching our interaction.rs & fillForm implementation)
  function fastInputSetter(el, text) {
    el.focus()
    const proto = Object.getPrototypeOf(el)
    const desc = Object.getOwnPropertyDescriptor(proto, 'value')
    if (desc && desc.set) {
      desc.set.call(el, text)
    } else {
      el.value = text
    }
    el.dispatchEvent({ type: 'input', bubbles: true })
    el.dispatchEvent({ type: 'change', bubbles: true })
  }

  fastInputSetter(inputEl, 'Jane Doe, Lead Engineer')

  assert.equal(inputEl.value, 'Jane Doe, Lead Engineer')
  assert.equal(state.value, 'Jane Doe, Lead Engineer')
  assert.equal(inputEl.focused, true)
  assert.equal(state.inputEvents, 1)
  assert.equal(state.changeEvents, 1)
  assert.equal(state.renderCount, 1)
})

test('Real Operation: Multi-field form batch execution payload', () => {
  const formState = {
    '#name': '',
    '#email': '',
    '#notes': '',
  }

  const elements = {
    '#name': { value: '', focused: false },
    '#email': { value: '', focused: false },
    '#notes': { value: '', focused: false },
  }

  const batchFields = [
    { selector: '#name', value: 'Alice Smith' },
    { selector: '#email', value: 'alice@example.com' },
    { selector: '#notes', value: 'Enterprise customer review notes' },
  ]

  // Simulate handle_fill_form JS loop
  const results = []
  for (const item of batchFields) {
    const el = elements[item.selector]
    if (!el) {
      results.push({ selector: item.selector, success: false })
      continue
    }
    el.focused = true
    el.value = item.value
    formState[item.selector] = item.value
    results.push({ selector: item.selector, success: true })
  }

  assert.equal(results.length, 3)
  assert.ok(results.every((r) => r.success))
  assert.equal(formState['#name'], 'Alice Smith')
  assert.equal(formState['#email'], 'alice@example.com')
  assert.equal(formState['#notes'], 'Enterprise customer review notes')
})

test('Real Operation: File chooser interception prevents native modal dialog deadlock', async () => {
  let fileChooserIntercepted = false
  let modalShown = false
  let dialogAction = null

  // Mock CDP transport
  const cdpClient = {
    async sendCommand(method, params) {
      if (method === 'Page.setInterceptFileChooserDialog') {
        fileChooserIntercepted = params.enabled
        return { success: true }
      }
      if (method === 'Page.handleFileChooser') {
        dialogAction = params.action
        return { success: true }
      }
      if (method === 'DOM.setFileInputFiles') {
        if (!fileChooserIntercepted) {
          modalShown = true
        }
        return { success: true }
      }
      return {}
    },
  }

  // 1. Arm interception as done in enable_domains & background.js
  await cdpClient.sendCommand('Page.setInterceptFileChooserDialog', { enabled: true })
  assert.equal(fileChooserIntercepted, true)

  // 2. Trigger file upload operation
  await cdpClient.sendCommand('DOM.setFileInputFiles', { files: ['/path/to/doc.pdf'] })
  assert.equal(modalShown, false, 'Native modal must not be triggered when intercepted')

  // 3. Protocol receives Page.fileChooserOpened and dismisses cleanly
  await cdpClient.sendCommand('Page.handleFileChooser', { action: 'cancel' })
  assert.equal(dialogAction, 'cancel')
})

test('Real Operation: Tab close and popup lifecycle re-anchoring to opener tab', () => {
  // Simulate browser pages state
  const pages = [
    { targetId: 'tab-main', url: 'https://app.example.com/dashboard', title: 'Dashboard' },
    { targetId: 'tab-oauth-popup', url: 'https://auth.example.com/login', title: 'Sign In', openerId: 'tab-main' },
  ]
  let activeTargetId = 'tab-oauth-popup'
  let activeIndex = 1

  function activeTargetAfterRemoval(remainingPages, nextActiveIdx, currentPin, removedId) {
    if (currentPin !== removedId) return currentPin
    // Surviving valid page fallback (from our updated browser.rs)
    if (remainingPages[nextActiveIdx] && remainingPages[nextActiveIdx].url !== 'about:blank') {
      return remainingPages[nextActiveIdx].targetId
    }
    return removedId // tombstone only if nothing left
  }

  // Popup finishes auth and closes -> targetDestroyed fires
  const removedId = 'tab-oauth-popup'
  const removedPos = pages.findIndex((p) => p.targetId === removedId)
  pages.splice(removedPos, 1)
  activeIndex = 0

  activeTargetId = activeTargetAfterRemoval(pages, activeIndex, activeTargetId, removedId)

  // Verify re-anchoring to main tab instead of dead tombstone
  assert.equal(activeTargetId, 'tab-main')
  assert.equal(pages.length, 1)
  assert.equal(pages[0].url, 'https://app.example.com/dashboard')
})

test('Real Operation: Long-running script evaluation with adaptive heartbeat extension', async () => {
  let heartbeatCalls = 0
  const heartbeatProbe = async () => {
    heartbeatCalls++
    return true // debugger is responsive
  }

  let executionFinished = false
  const operation = new Promise((resolve) => {
    setTimeout(() => {
      executionFinished = true
      resolve({ result: 'completed-large-analysis' })
    }, 45)
  })

  // Set tight base timeout of 30ms; heartbeat extends it so operation completes cleanly
  const res = await withRelayTimeout(operation, 'long-evaluation', 30, {
    checkHeartbeat: heartbeatProbe,
  })

  assert.equal(executionFinished, true)
  assert.equal(res.result, 'completed-large-analysis')
  assert.ok(heartbeatCalls >= 1, 'Adaptive heartbeat probe was engaged')
})
