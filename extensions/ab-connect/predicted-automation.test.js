import test from 'node:test';
import assert from 'node:assert/strict';
import {
  evaluateAutomationTriggers,
  executeAutomationStep,
  executePredictedAutomation,
} from './predicted-automation.js';

class MockElement {
  constructor(tagName, id = '', className = '') {
    this.tagName = tagName.toUpperCase();
    this.id = id;
    this.className = className;
    this.value = '';
    this.textContent = '';
    this.innerText = '';
    this.offsetWidth = 100;
    this.offsetHeight = 30;
    this.events = [];
    this.clicked = false;
  }

  getClientRects() {
    return [{ width: this.offsetWidth, height: this.offsetHeight }];
  }

  dispatchEvent(event) {
    this.events.push(event.type);
    return true;
  }

  focus() {
    this.events.push('focus');
  }

  click() {
    this.clicked = true;
    this.events.push('click');
  }

  scrollIntoView() {}
}

class MockDocument {
  constructor() {
    this.elements = new Map();
  }

  register(selector, el) {
    this.elements.set(selector, el);
  }

  querySelector(selector) {
    return this.elements.get(selector) || null;
  }
}

test('Predicted Automation: Successful execution without divergence', async () => {
  const doc = new MockDocument();
  const searchInput = new MockElement('input', 'search-box');
  const submitBtn = new MockElement('button', 'submit-btn');
  const resultsContainer = new MockElement('div', 'results');

  doc.register('#search-box', searchInput);
  doc.register('#submit-btn', submitBtn);
  doc.register('#results', resultsContainer);

  const automation = {
    id: 'search-flow',
    preconditions: [
      { selector: '#search-box', visible: true },
      { selector: '#submit-btn', visible: true },
    ],
    antiTriggers: [
      { selector: '.captcha-frame', description: 'Captcha blocking automation' },
      { selector: '.error-alert', description: 'Error alert present' },
    ],
    steps: [
      { action: 'fill', selector: '#search-box', value: 'chrome-use' },
      { action: 'click', selector: '#submit-btn' },
    ],
    postConditionSelector: '#results',
  };

  const result = await executePredictedAutomation(automation, doc);
  assert.equal(result.success, true);
  assert.equal(result.diverged, false);
  assert.equal(result.stepsExecuted, 2);
  assert.equal(searchInput.value, 'chrome-use');
  assert.equal(submitBtn.clicked, true);
});

test('Predicted Automation Divergence: Anti-trigger detects captcha or modal overlay', async () => {
  const doc = new MockDocument();
  const searchInput = new MockElement('input', 'search-box');
  const captcha = new MockElement('iframe', 'captcha-frame');

  doc.register('#search-box', searchInput);
  doc.register('.captcha-frame', captcha);

  const automation = {
    id: 'search-flow',
    preconditions: [{ selector: '#search-box', visible: true }],
    antiTriggers: [
      { selector: '.captcha-frame', description: 'Cloudflare / Recaptcha challenge detected' },
    ],
    steps: [{ action: 'fill', selector: '#search-box', value: 'chrome-use' }],
  };

  const result = await executePredictedAutomation(automation, doc);
  assert.equal(result.success, false);
  assert.equal(result.diverged, true);
  assert.equal(result.divergenceType, 'anti_trigger');
  assert.match(result.reason, /challenge detected/);
  // Ensure steps were NOT executed
  assert.equal(searchInput.value, '');
});

test('Predicted Automation Divergence: Precondition selector missing yields control to AI', async () => {
  const doc = new MockDocument();
  // search box is NOT in the DOM
  const automation = {
    id: 'search-flow',
    preconditions: [{ selector: '#search-box', visible: true }],
    steps: [{ action: 'fill', selector: '#search-box', value: 'chrome-use' }],
  };

  const result = await executePredictedAutomation(automation, doc);
  assert.equal(result.success, false);
  assert.equal(result.diverged, true);
  assert.equal(result.divergenceType, 'precondition_missing');
  assert.match(result.reason, /Required precondition selector not found/);
});

test('Predicted Automation Divergence: Precondition text mismatch yields control to AI', async () => {
  const doc = new MockDocument();
  const header = new MockElement('h1', 'page-title');
  header.textContent = 'Welcome to Old Portal';
  doc.register('#page-title', header);

  const automation = {
    id: 'portal-login',
    preconditions: [{ selector: '#page-title', text: 'New Modern Portal' }],
    steps: [{ action: 'click', selector: '#login' }],
  };

  const result = await executePredictedAutomation(automation, doc);
  assert.equal(result.success, false);
  assert.equal(result.diverged, true);
  assert.equal(result.divergenceType, 'precondition_text_mismatch');
  assert.match(result.reason, /Precondition text mismatch/);
});

test('Predicted Automation Divergence: Step execution failure halts pipeline gracefully', async () => {
  const doc = new MockDocument();
  const btn1 = new MockElement('button', 'step-1');
  doc.register('#step-1', btn1);
  // step-2 is missing from DOM

  const automation = {
    id: 'multi-step',
    steps: [
      { action: 'click', selector: '#step-1' },
      { action: 'click', selector: '#missing-step-2' },
    ],
  };

  const result = await executePredictedAutomation(automation, doc);
  assert.equal(result.success, false);
  assert.equal(result.diverged, true);
  assert.equal(result.divergenceType, 'step_execution_failure');
  assert.equal(result.stepIndex, 1);
  assert.equal(btn1.clicked, true);
});
