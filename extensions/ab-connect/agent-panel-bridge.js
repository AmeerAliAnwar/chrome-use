// Correlate extension-page requests with the native host, separately from CDP.
// The bridge neither attaches tabs nor executes page JavaScript. Native-host
// replies must match the request's connection generation; no request is replayed.
export class AgentPanelBridge {
  constructor({ send, schedule = setTimeout, cancel = clearTimeout, timeoutMs = 15000, maxPending = 16 }) {
    this.send = send;
    this.schedule = schedule;
    this.cancel = cancel;
    this.timeoutMs = timeoutMs;
    this.maxPending = maxPending;
    this.generation = 0;
    this.sequence = 0;
    this.identity = globalThis.crypto.randomUUID();
    this.pending = new Map();
  }

  request(operation, params = {}) {
    if (operation !== 'discover') return Promise.reject(new Error(`Unsupported agent panel operation: ${operation}`));
    if (this.pending.size >= this.maxPending) return Promise.reject(new Error('Too many pending agent panel requests'));
    const requestId = `${this.identity}:${this.generation}:${++this.sequence}`;
    return new Promise((resolve, reject) => {
      const timeout = this.schedule(() => {
        this.pending.delete(requestId);
        reject(new Error('The native host did not answer the agent panel request. Check the host version and connection; the request was not replayed.'));
      }, this.timeoutMs);
      this.pending.set(requestId, { resolve, reject, timeout });
      try { this.send({ method: 'agentPanel', requestId, operation, params }); }
      catch (error) {
        this.cancel(timeout);
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }

  receive(message) {
    if (message?.method !== 'agentPanelReply') return false;
    const pending = this.pending.get(message.requestId);
    if (!pending) return true; // Late/duplicate panel reply must never enter CDP.
    this.pending.delete(message.requestId);
    this.cancel(pending.timeout);
    if (typeof message.error === 'string') pending.reject(new Error(message.error));
    else if (message.result?.schemaVersion === 1 && Array.isArray(message.result.agents)) pending.resolve(message.result);
    else pending.reject(new Error('Malformed agent panel reply from the native host'));
    return true;
  }

  disconnect(reason) {
    this.generation++;
    for (const { reject, timeout } of this.pending.values()) {
      this.cancel(timeout);
      reject(new Error(reason || 'Native host disconnected; the request was not replayed'));
    }
    this.pending.clear();
  }
}

// A content script, another extension, or a website cannot invoke local panel
// services. Check sender origin and the actual extension page URL, not a
// caller-supplied operation or claimed identity.
export function isAgentPanelSender(sender, runtime) {
  if (sender?.id !== runtime.id || sender.tab) return false;
  return [runtime.getURL('sidepanel.html'), runtime.getURL('options.html')].includes(sender.url);
}
