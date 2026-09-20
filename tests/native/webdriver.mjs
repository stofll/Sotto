// A small W3C client for the spike. No Tauri service, IPC mocks or frontend plugin.
export class WebDriver {
  constructor(port, fetchImpl = fetch) {
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Invalid driver port');
    this.url = `http://127.0.0.1:${port}`;
    this.fetch = fetchImpl;
    this.session = null;
  }

  async request(method, route, body) {
    const response = await this.fetch(this.url + route, {
      method,
      headers: { 'Content-Type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: AbortSignal.timeout(5000),
      redirect: 'error',
    });
    const payload = await response.json();
    if (!response.ok || payload.value?.error) throw new Error(`WebDriver ${route}: ${payload.value?.error ?? response.status}: ${payload.value?.message ?? ''}`);
    return payload.value;
  }

  async start() {
    const value = await this.request('POST', '/session', { capabilities: { alwaysMatch: { 'wdio:tauriServiceOptions': { windowLabel: 'main' } } } });
    if (!value?.sessionId) throw new Error('WebDriver did not return a session ID');
    this.session = encodeURIComponent(value.sessionId);
  }

  command(method, suffix, body) {
    if (!this.session) throw new Error('No driver session');
    return this.request(method, `/session/${this.session}${suffix}`, body);
  }

  read(script) { return this.command('POST', '/execute/sync', { script, args: [] }); }
  readAsync(script) { return this.command('POST', '/execute/async', { script, args: [] }); }
  handles() { return this.command('GET', '/window/handles'); }
  switchTo(handle) { return this.command('POST', '/window', { handle }); }
  screenshot() { return this.command('GET', '/screenshot'); }

  async click(selector) {
    const element = await this.command('POST', '/element', { using: 'css selector', value: selector });
    const id = element?.['element-6066-11e4-a52e-4f735466cecf'];
    if (!id) throw new Error(`Element not found: ${selector}`);
    await this.command('POST', `/element/${encodeURIComponent(id)}/click`, {});
  }
}
