import assert from 'node:assert/strict';
import test from 'node:test';
import { createPanel } from './sidepanel.js';

// A deliberately small DOM fixture: controller tests verify asynchronous
// conversation identity, not browser layout or extension API compatibility.
class Node {
  constructor(tag = 'div') {
    this.tag = tag; this.children = []; this.listeners = {}; this.dataset = {};
    this.value = ''; this.textContent = ''; this.hidden = false; this.disabled = false;
    this.scrollHeight = 100; this.clientHeight = 100; this.scrollTop = 0;
    this.classList = { toggle() {} };
  }
  append(...nodes) { for (const node of nodes) { node.parent = this; this.children.push(node); } }
  insertBefore(node, before) { if (node.parent) node.remove(); node.parent=this; const index=this.children.indexOf(before); this.children.splice(index<0?this.children.length:index,0,node); }
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  get firstElementChild() { return this.children[0]; }
  addEventListener(type, listener) { this.listeners[type] = listener; }
  setAttribute() {}
  querySelectorAll(tag) { return this.children.flatMap(node => [ ...(node.tag === tag ? [node] : []), ...node.querySelectorAll(tag)]); }
  remove() { this.parent.children = this.parent.children.filter(node => node !== this); }
  fire(type) { this.listeners[type]?.({ preventDefault() {} }); }
}
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const settle = () => new Promise(resolve => setImmediate(resolve));
const chat = (id, phase = 'idle') => ({ id, title: id, phase, queued: 0, queuePaused: false, model: '' });

async function fixture(overrides = {}, saved = {}) {
  const nodes = new Map(), calls = [], writes = [];
  const chats = [chat('a', 'running'), chat('b')];
  const runtime = {
    onMessage: { addListener() {} },
    async sendMessage(request) {
      if (request.type === 'ab-status') return { connected: true };
      calls.push(request);
      if (overrides[request.operation]) return overrides[request.operation](request.params);
      let result;
      switch (request.operation) {
        case 'discover': result = { agents: [] }; break;
        case 'conversationList': result = { conversations: chats }; break;
        case 'conversationRead': result = { conversation: chats.find(c => c.id === request.params.conversationId), journal: { events: [], cursor: 0, more: false } }; break;
        case 'conversationMessageStatus': result = { message: { known: false } }; break;
        default: throw new Error(`Unexpected operation ${request.operation}`);
      }
      return { success: true, result };
    },
  };
  const document = { createElement: tag => new Node(tag), getElementById(id) { if (!nodes.has(id)) nodes.set(id, new Node()); return nodes.get(id); } };
  const panel = createPanel({ document, runtime, storage: { local: { async get() { return saved; }, async set(values) { writes.push(structuredClone(values)); } } }, schedule() {}, cancel() {} });
  await panel.start(); await settle();
  return { panel, nodes, calls, chats, writes };
}

test('a late stop acknowledgement cannot replace the selected conversation', async () => {
  const stop = deferred();
  const f = await fixture({ conversationStop: () => stop.promise });
  f.panel.select('a'); await settle(); f.nodes.get('stop').fire('click');
  f.panel.select('b'); await settle();
  stop.resolve({ success: true, result: { conversation: chat('a', 'stopping') } }); await settle();
  assert.equal(f.nodes.get('phase').textContent, 'Ready');
  assert.equal(f.nodes.get('conversation').value, 'b');
  f.panel.close();
});

test('sending locks selection until the acknowledgement and keeps the message identity on failure', async () => {
  const send = deferred();
  const f = await fixture({ conversationSend: () => send.promise });
  f.panel.select('a'); await settle(); f.nodes.get('message').value = 'fixture message';
  const sending = f.panel.send();
  assert.equal(f.nodes.get('conversation').disabled, true);
  f.panel.select('b'); assert.equal(f.nodes.get('conversation').value, 'a');
  send.reject(new Error('Host disconnected')); await assert.rejects(sending, /disconnected/);
  assert.equal(f.nodes.get('conversation').disabled, false);
  assert.equal(f.nodes.get('send').disabled, true);
  assert.match(f.nodes.get('queue').textContent, /unconfirmed/);
  f.panel.select('b'); await settle(); assert.equal(f.nodes.get('send').disabled, false);
  f.panel.select('a'); await settle(); assert.equal(f.nodes.get('send').disabled, true);
  const message = f.calls.find(c => c.operation === 'conversationSend');
  const status = f.calls.find(c => c.operation === 'conversationMessageStatus');
  assert.equal(status.params.messageId, message.params.messageId);
  await f.panel.send(); assert.equal(f.calls.filter(c => c.operation === 'conversationSend').length, 1);
  f.panel.close();
});

test('switching conversations preserves each unsent draft', async () => {
  const f = await fixture(); f.panel.select('a'); await settle();
  f.nodes.get('message').value = 'draft a'; f.panel.select('b'); await settle();
  assert.equal(f.nodes.get('message').value, ''); f.nodes.get('message').value = 'draft b';
  f.panel.select('a'); await settle(); assert.equal(f.nodes.get('message').value, 'draft a');
  f.panel.select('b'); await settle(); assert.equal(f.nodes.get('message').value, 'draft b'); f.panel.close();
});

test('an old transcript reply is discarded after conversation selection changes', async () => {
  const read = deferred();
  const f = await fixture({ conversationRead: ({ conversationId }) => conversationId === 'a' ? read.promise : Promise.resolve({ success: true, result: { conversation: chat('b'), journal: { events: [], cursor: 0, more: false } } }) });
  f.panel.select('a'); f.panel.select('b');
  read.resolve({ success: true, result: { conversation: chat('a'), journal: { events: [{ sequence: 1, kind: 'user', data: { messageId: 'old', text: 'old chat' } }], cursor: 1, more: false } } }); await settle();
  assert.equal(f.panel.timeline().rows.length, 0); assert.equal(f.nodes.get('conversation').value, 'b'); f.panel.close();
});


test('reopening a panel checks an uncertain stored ID without resending it', async () => {
  const saved = { ab_panel_conversation: 'a', ab_panel_pending: { a: 'original-message-id' } };
  const f = await fixture({}, saved);
  assert.equal(f.nodes.get('send').disabled, true);
  assert.match(f.nodes.get('queue').textContent, /unconfirmed/);
  assert.ok(f.calls.some(c => c.operation === 'conversationMessageStatus' && c.params.messageId === 'original-message-id'));
  f.nodes.get('message').value = 'do not resend'; await f.panel.send();
  assert.equal(f.calls.filter(c => c.operation === 'conversationSend').length, 0); f.panel.close();
});

test('a durable acceptance clears only the stored uncertain ID', async () => {
  const f = await fixture({ conversationMessageStatus: async () => ({ success: true, result: { message: { known: true } } }) }, { ab_panel_conversation: 'a', ab_panel_pending: { a: 'accepted', b: 'other' } });
  assert.ok(f.writes.some(w => w.ab_panel_pending?.b === 'other' && !w.ab_panel_pending.a));
  assert.equal(f.nodes.get('send').disabled, false);
  assert.equal(f.calls.filter(c => c.operation === 'conversationSend').length, 0); f.panel.close();
});

test('editing a queued message preserves the composer draft and sends its original text for conflict detection', async () => {
  let queued = [{ id: 'queued', text: 'original', truncated: false }, { id: 'second', text: 'second message' }];
  const f = await fixture({
    conversationRead: async () => ({ success:true,result:{ conversation:chat('a','running'), journal:{events:[],cursor:0,more:false},queuedMessages:queued } }),
    conversationQueueEdit: async params => { queued=[{id:params.replacementId,text:params.text},queued[1]]; return {success:true,result:{}}; },
  });
  f.panel.select('a'); await settle(); f.nodes.get('message').value='separate draft';
  const section=f.nodes.get('queuedMessages'); const input=section.querySelectorAll('textarea')[0]; input.value='replacement';
  section.querySelectorAll('button')[0].fire('click'); await settle();
  const edit=f.calls.find(call=>call.operation==='conversationQueueEdit');
  assert.equal(edit.params.messageId,'queued'); assert.equal(edit.params.expectedText,'original');
  assert.equal(edit.params.text,'replacement'); assert.notEqual(edit.params.replacementId,'queued');
  assert.equal(f.nodes.get('message').value,'separate draft');
  assert.equal(section.querySelectorAll('textarea')[0].value,'replacement');
  assert.equal(section.querySelectorAll('textarea')[1].value,'second message'); f.panel.close();
});

test('a truncated queue preview cannot be changed until its full text is read', async () => {
  const full='x'.repeat(1500);
  const f=await fixture({
    conversationRead:async()=>({success:true,result:{conversation:chat('a'),journal:{events:[],cursor:0,more:false},queuedMessages:[{id:'large',text:full.slice(0,1024),truncated:true}]}}),
    conversationQueueMessageRead:async()=>({success:true,result:{queuedMessage:{id:'large',text:full}}}),
  });
  f.panel.select('a'); await settle();
  const section=f.nodes.get('queuedMessages'); const input=section.querySelectorAll('textarea')[0];
  assert.equal(input.readOnly,true);
  section.querySelectorAll('button')[0].fire('click'); await settle();
  assert.equal(input.value,full); assert.equal(input.readOnly,false);
  assert.equal(f.calls.filter(c=>c.operation==='conversationQueueEdit').length,0); f.panel.close();
});

test('direct sending is acknowledged as waiting for stop, and cannot replace a pending direct message', async () => {
  const reply=deferred();
  let current=chat('a','running');
  const f=await fixture({conversationSendDirect:()=>reply.promise,conversationRead:async()=>({success:true,result:{conversation:current,journal:{events:[],cursor:0,more:false},queuedMessages:current.directMessageId?[{id:current.directMessageId,text:'send after stop',truncated:false,direct:true}]:[]}})});
  f.panel.select('a');await settle();f.nodes.get('message').value='send after stop';
  const sending=f.panel.send(true);await settle();
  assert.equal(f.nodes.get('sendDirect').disabled,true);
  const request=f.calls.find(c=>c.operation==='conversationSendDirect');
  assert.ok(f.writes.some(w=>w.ab_panel_pending?.a===request.params.messageId));
  current={...chat('a','stopping'),directMessageId:request.params.messageId,queuePaused:true,queued:1};
  reply.resolve({success:true,result:{conversation:current}});
  await sending;
  assert.equal(f.nodes.get('phase').textContent,'Waiting for stop confirmation');
  assert.equal(f.nodes.get('sendDirect').disabled,true);
  f.nodes.get('message').value='do not replace the first direct message';
  await assert.rejects(f.panel.send(true),/already waiting/);
  assert.equal(f.calls.filter(c=>c.operation==='conversationStop').length,0);
  assert.equal(f.calls.filter(c=>c.operation==='conversationSendDirect').length,1);
  f.panel.close();
});
