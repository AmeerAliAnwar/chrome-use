#!/usr/bin/env node
// Functional extension fixture only. CDP observes extension surfaces in a
// separately owned test browser; this is not a stealth detector run.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';

const flags = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, index, args) => index % 2 === 0 ? [...pairs, [value, args[index + 1]]] : pairs, []));
const state = JSON.parse(await readFile(flags['--state-file'], 'utf8'));
const extensionId = 'ciiljdlhdpfckdcfkphgmfalanpdejep';
const panelUrl = `chrome-extension://${extensionId}/sidepanel.html`;
const socket = new WebSocket(state.browserWebSocket);
await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once:true }); socket.addEventListener('error', reject, { once:true }); });
let sequence = 0;
const pending = new Map(), evidence = { purpose:'extension-functional-fixture', remoteDebugging:true, steps:[] };
socket.addEventListener('message', event => {
  const reply = JSON.parse(String(event.data));
  const request = pending.get(reply.id); if (!request) return;
  pending.delete(reply.id); clearTimeout(request.timer);
  reply.error ? request.reject(new Error(`${request.method}: ${reply.error.message}`)) : request.resolve(reply.result);
});
function call(method, params = {}, sessionId) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Timed out: ${method}`)); }, 15000);
    pending.set(id, { resolve, reject, timer, method });
    socket.send(JSON.stringify({id,method,params,...(sessionId ? {sessionId} : {})}));
  });
}
async function evaluate(sessionId, expression, userGesture = false) {
  const result = await call('Runtime.evaluate', {expression,awaitPromise:true,returnByValue:true,userGesture}, sessionId);
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
  return result.result.value;
}
async function waitFor(read, accept, label) {
  const deadline = Date.now() + 15000;
  let value;
  while (Date.now() < deadline) { value = await read(); if (accept(value)) return value; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label}: ${JSON.stringify(value)}`);
}
async function target(predicate, label) {
  return waitFor(async () => (await call('Target.getTargets')).targetInfos.find(predicate), Boolean, label);
}
let panelSession;
try {
  evidence.browser = (await call('Browser.getVersion')).product;
  const worker = await target(t => t.type === 'service_worker' && t.url === `chrome-extension://${extensionId}/background.js`, 'Extension worker did not load');
  const workerSession = (await call('Target.attachToTarget', {targetId:worker.targetId,flatten:true})).sessionId;
  await call('Runtime.enable', {}, workerSession);
  await waitFor(() => evaluate(workerSession, 'typeof chrome'), value => value === 'object', 'Extension bindings did not initialize');
  await evaluate(workerSession, `globalThis.__panelSenders=[]; chrome.runtime.onMessage.addListener((message,sender)=>{if(message?.type==='ab-agent-request')__panelSenders.push({operation:message.operation,panel:sender.url===${JSON.stringify(panelUrl)},hasTab:Boolean(sender.tab)});}); true`);
  const entry = await waitFor(() => evaluate(workerSession, `(async()=>({behavior:await chrome.sidePanel.getPanelBehavior(),popup:await chrome.action.getPopup({})}))()`), value => value?.behavior?.openPanelOnActionClick === true && value.popup === '', 'Toolbar behavior not confirmed');
  evidence.steps.push({entry});
  const windowId = await evaluate(workerSession, '(async()=>(await chrome.windows.getAll())[0].id)()');
  const optionsTarget = await call('Target.createTarget', {url:`chrome-extension://${extensionId}/options.html`});
  const optionsSession = (await call('Target.attachToTarget', {targetId:optionsTarget.targetId,flatten:true})).sessionId;
  await call('Runtime.enable', {}, optionsSession);
  await waitFor(() => evaluate(optionsSession, 'typeof chrome'), value => value === 'object', 'Options document bindings did not initialize');
  await evaluate(optionsSession, `(()=>{const button=document.createElement('button');button.textContent='Open fixture panel';button.style='position:fixed;left:10px;top:10px;width:160px;height:40px;z-index:2147483647';button.addEventListener('click',()=>{chrome.sidePanel.open({windowId:${windowId}}).then(()=>globalThis.__fixturePanelOpened=true,error=>globalThis.__fixturePanelError=error.message);});document.body.append(button);return true;})()`);
  await call('Input.dispatchMouseEvent', {type:'mousePressed',x:40,y:30,button:'left',clickCount:1}, optionsSession);
  await call('Input.dispatchMouseEvent', {type:'mouseReleased',x:40,y:30,button:'left',clickCount:1}, optionsSession);
  await waitFor(() => evaluate(optionsSession, '({opened:globalThis.__fixturePanelOpened,error:globalThis.__fixturePanelError})'), value => value.opened, 'Trusted extension-page gesture did not open the panel');
  evidence.steps.push({panelOpenGesture:'trusted extension-page click'});
  const panel = await target(t => t.url === panelUrl, 'Native side panel target did not appear');
  evidence.panelTargetType = panel.type;
  panelSession = (await call('Target.attachToTarget', {targetId:panel.targetId,flatten:true})).sessionId;
  const initial = await waitFor(() => evaluate(panelSession, `({connection:document.getElementById('connection')?.textContent,agentCount:document.getElementById('agentList')?.children.length,notice:document.getElementById('notice')?.textContent})`), value => value?.connection === 'Local bridge connected' && value.agentCount >= 2, 'Panel did not load native discovery');
  evidence.steps.push({initial});
  await evaluate(panelSession, `document.getElementById('executable').value=${JSON.stringify(flags['--fixture-agent'])};document.getElementById('executable').dispatchEvent(new Event('input'));document.getElementById('inspectAgent').click();true`);
  const catalog = await waitFor(() => evaluate(panelSession, `({count:document.getElementById('modelList').children.length,status:document.getElementById('modelStatus').textContent,notice:document.getElementById('notice').textContent})`), value => value.count === 2, 'Model catalog did not arrive through the real extension');
  assert.match(catalog.status, /execution access is unverified/);
  evidence.steps.push({catalog});
  await evaluate(panelSession, `document.getElementById('message').value='first';document.getElementById('composer').requestSubmit();true`);
  const chat = await waitFor(() => evaluate(panelSession, `({phase:document.getElementById('phase').textContent,text:document.getElementById('transcript').textContent,notice:document.getElementById('notice').textContent})`), value => value.phase === 'Ready' && value.text.includes('synthetic reply: first'), 'Conversation did not complete');
  evidence.steps.push({chat});
  evidence.senders = await evaluate(workerSession, '__panelSenders');
  assert.ok(evidence.senders.some(sender => sender.operation === 'conversationSend' && sender.panel));
  evidence.geometry = await evaluate(panelSession, `({width:innerWidth,height:innerHeight,bodyWidth:document.body.scrollWidth,bodyHeight:document.body.scrollHeight})`);
  assert.ok(evidence.geometry.bodyWidth <= evidence.geometry.width, 'Panel has horizontal overflow');
  const screenshot = await call('Page.captureScreenshot', {format:'png'}, panelSession);
  await writeFile(flags['--output'] + '.png', Buffer.from(screenshot.data, 'base64'));
  const stopped = await evaluate(panelSession, `(async()=>await chrome.runtime.sendMessage({type:'ab-agent-request',operation:'serviceStop',params:{}}))()`);
  assert.equal(stopped.success, true);
  evidence.serviceStopAccepted = true;
  evidence.verdict = 'passed';
} catch (error) {
  evidence.verdict = 'failed'; evidence.error = error.message;
  if (panelSession) {
    try { evidence.panelState = await evaluate(panelSession, `({text:document.body.innerText,notice:document.getElementById('notice')?.textContent})`); } catch {}
    try { await evaluate(panelSession, `(async()=>await chrome.runtime.sendMessage({type:'ab-agent-request',operation:'serviceStop',params:{}}))()`); } catch {}
  }
  process.exitCode = 1;
} finally {
  await writeFile(flags['--output'], JSON.stringify(evidence,null,2)+'\n');
  console.log(JSON.stringify(evidence));
  socket.close();
}
