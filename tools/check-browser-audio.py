#!/usr/bin/env python3
"""Check real browser audio for every bundled plugin across cold, warm and offline starts.

Requires Python 3.11+, Playwright and Chrome/Chromium. Runs a temporary local
server and an isolated headless browser profile; never opens the user's desktop.
Instruments receive MIDI notes in three registers. Effects process Concert Grand
in PLAY. Use a demo fixture containing all official packages to include Soundfonts.

Example:
    python tools/check-browser-audio.py --site /path/to/browser-dist --output /tmp/browser-audio.json
"""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
import argparse, json, shutil, time, tomllib
from playwright.sync_api import sync_playwright
WORKLET_PROBE = r"""
(() => {
  Object.defineProperty(navigator, "requestMIDIAccess", { value: undefined });
  const connect = AudioNode.prototype.connect;
  let nextId = 1000000;
  AudioNode.prototype.connect = function(destination, ...args) {
    if (this instanceof AudioWorkletNode && destination === this.context.destination) {
      const port = this.port;
      window.probeRequest = request => new Promise((resolve, reject) => {
        const id = nextId++;
        const onMessage = ({data}) => {
          if (data.kind !== "response" || data.id !== id) return;
          clearTimeout(timer);
          port.removeEventListener("message", onMessage);
          resolve(JSON.parse(data.response));
        };
        const timer = setTimeout(() => {
          port.removeEventListener("message", onMessage);
          reject(new Error("RackForge request timed out: " + request.op));
        }, 5000);
        port.addEventListener("message", onMessage);
        port.postMessage({kind:"request", id, request:JSON.stringify(request)});
      });
      window.probeCc = (number, value) => port.postMessage({
        kind:"controller_midi", data:[0xb0, number, value], length:3
      });
    }
    return connect.call(this, destination, ...args);
  };
})();
"""

class Handler(SimpleHTTPRequestHandler):

    def end_headers(self):
        self.send_header('Cache-Control', 'public, max-age=600')
        super().end_headers()

    def log_message(self, *args):
        pass

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', type=Path, required=True, help='Built browser demo, including demo/storage.json and extracted plugin packages')
    parser.add_argument('--output', type=Path, required=True, help='JSON validation receipt')
    parser.add_argument('--chrome', help='Chrome or Chromium executable; tests always run headlessly')
    parser.add_argument('--require-parallel', action='store_true', help='Require warm-cache audio to advance instance-owned worker pools (including Concert Grand reports)')
    parser.add_argument('--delay-worker-ms', type=int, default=0, help='Inject one delayed Concert Grand worker per warm phase, testing queued MIDI and report recovery')
    args = parser.parse_args()
    site = args.site.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for required in ('index.html', 'sw.js', 'demo/storage.json', 'demo/rackforge-browser.wasm'):
        if not (site / required).is_file():
            parser.error(f'Missing built demo file: {required}')
    chrome = args.chrome or shutil.which('google-chrome') or shutil.which('chromium') or 'C:/Program Files/Google/Chrome/Application/chrome.exe'
    server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(site)))
    Thread(target=server.serve_forever, daemon=True).start()
    origin = f'http://rackforge-cache.test:{server.server_port}'
    plugins = [tomllib.loads(p.read_text()) for p in (site / 'demo/rackforge/plugins').glob('*/rackforge-plugin.toml')]
    results = []
    switches = []
    logs = []
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(executable_path=chrome, headless=True, args=['--mute-audio', '--autoplay-policy=no-user-gesture-required', '--no-proxy-server', '--host-resolver-rules=MAP rackforge-cache.test 127.0.0.1', f'--unsafely-treat-insecure-origin-as-secure={origin}'])
            context = browser.new_context(viewport={'width': 1500, 'height': 1000})
            context.add_init_script(WORKLET_PROBE)
            if args.delay_worker_ms:
                context.add_init_script(r'''
                    if (crossOriginIsolated) {
                        const gate = window.testWorkerDelay = new Int32Array(new SharedArrayBuffer(4));
                        const NativeWorker = Worker;
                        window.Worker = class extends NativeWorker {
                            constructor(url, options) {
                                if (!String(url).includes('render.worker')) { super(url, options); return; }
                                const code = `import ${JSON.stringify(String(url))};
                                    let gate;
                                    const receive = self.onmessage;
                                    self.onmessage = e => { gate = e.data.testGate; receive(e); };
                                    const wait = Atomics.wait;
                                    Atomics.wait = (...args) => {
                                        const result = wait(...args);
                                        if (gate) {
                                            const ms = Atomics.exchange(gate, 0, 0);
                                            const until = Date.now() + ms;
                                            while (Date.now() < until) { /* deterministic scheduling delay */ }
                                        }
                                        return result;
                                    };`;
                                const blob = URL.createObjectURL(new Blob([code], {type:'text/javascript'}));
                                super(blob, options);
                            }
                            postMessage(message, ...args) {
                                super.postMessage(message.geometry?.reportStride ? {...message, testGate:gate} : message, ...args);
                            }
                        };
                    }
                ''')
            page = context.new_page()
            page.set_default_timeout(120000)
            page.on('console', lambda m: logs.append({'type': m.type, 'text': m.text}))
            page.on('pageerror', lambda e: logs.append({'type': 'pageerror', 'text': str(e)}))

            def request(message):
                return page.evaluate('message=>window.probeRequest(message)', message)

            def command(value):
                snapshot = request({'op': 'snapshot'})['snapshot']
                reply = request({'op': 'dispatch', 'envelope': {'schema_version': snapshot['schema_version'], 'client_id': 'test.all-cache', 'command_id': int(time.monotonic() * 1000000), 'command': value}})
                assert reply['status'] == 'command_applied', reply

            def pools():
                return page.evaluate('''() => [...(window.__rfRenderPool?.pools ?? [])].map(([instance,p]) => ({
                    instance, epoch:p.epoch, workers:p.workers.length, geometry:p.geometry,
                    blocks:Atomics.load(p.control,10), deferred:Atomics.load(p.control,11),
                    misses:Atomics.load(p.control,8), ready:Atomics.load(p.control,6)
                }))''')

            def audio():
                for note in (48, 60, 72):
                    request({'op': 'virtual_midi', 'client_id': 'test.all-cache.keys', 'message': {'status': 144, 'data1': note, 'data2': 100}})
                peak = 0.0
                nonzero = 0
                for _ in range(16):
                    page.wait_for_timeout(40)
                    meter = request({'op': 'output_meter'})['meter']
                    level = max(meter['left_peak'], meter['right_peak'])
                    peak = max(peak, level)
                    nonzero += level > 1e-05
                request({'op': 'release_virtual_midi', 'client_id': 'test.all-cache.keys'})
                return (peak, nonzero)

            def check_switches(snapshot, phase):
                instruments = [i for i in snapshot['instances'] if any(m['id'] == i['plugin_id'] and m['kind'] == 'instrument' for m in plugins)]
                for index, source in enumerate(instruments):
                    other = instruments[(index + 1) % len(instruments)]
                    command({'type': 'select_plugin', 'instance_id': source['instance_id']})
                    for note in (48, 60, 72):
                        request({'op': 'virtual_midi', 'client_id': 'test.switch.keys', 'message': {'status': 144, 'data1': note, 'data2': 100}})
                    before_peak = 0
                    for _ in range(4):
                        page.wait_for_timeout(40)
                        meter = request({'op': 'output_meter'})['meter']
                        before_peak = max(before_peak, meter['left_peak'], meter['right_peak'])
                    ownership = [(p['instance'], p['epoch']) for p in pools()]
                    command({'type': 'select_plugin', 'instance_id': other['instance_id']})
                    page.wait_for_timeout(80)
                    command({'type': 'select_plugin', 'instance_id': source['instance_id']})
                    # No retrigger: only the voices that existed before the switch
                    # can produce this audio. Ignore the first meter window.
                    page.wait_for_timeout(80)
                    levels = []
                    for _ in range(6):
                        page.wait_for_timeout(40)
                        meter = request({'op': 'output_meter'})['meter']
                        levels.append(max(meter['left_peak'], meter['right_peak']))
                    request({'op': 'release_virtual_midi', 'client_id': 'test.switch.keys'})
                    after = [(p['instance'], p['epoch']) for p in pools()]
                    record = {'phase': phase, 'plugin': source['plugin_id'], 'via': other['plugin_id'], 'before_peak': before_peak, 'resumed_peak': max(levels), 'nonzero_windows': sum(x > 1e-5 for x in levels), 'ownership_preserved': ownership == after}
                    switches.append(record)
                    print('HELD_VOICE_SWITCH', json.dumps(record), flush=True)
                    args.output.write_text(json.dumps({'results': results, 'switches': switches, 'logs': logs}, indent=2))
                    assert record['before_peak'] > 1e-5 and record['nonzero_windows'] >= 3 and record['ownership_preserved'], record
            for phase in ('cold', 'warm', 'warm-repeat', 'offline'):
                print('PHASE', phase, flush=True)
                if phase == 'cold':
                    page.goto(origin + '/', wait_until='domcontentloaded')
                else:
                    if phase == 'offline':
                        context.set_offline(True)
                    page.reload(wait_until='domcontentloaded')
                page.get_by_role('button', name='Start RackForge', exact=True).click()
                page.wait_for_function("typeof window.probeRequest==='function'", polling=100)
                page.locator('.play-plugin-toolbar').wait_for(state='visible')
                if phase != 'cold':
                    assert page.evaluate('crossOriginIsolated && !!navigator.serviceWorker.controller'), \
                        'Warm/offline checks must exercise the controlling production service worker'
                snapshot = request({'op': 'snapshot'})['snapshot']
                ids = {i['plugin_id'] for i in snapshot['instances']}
                assert {m['id'] for m in plugins} <= ids, {'missing': sorted({m['id'] for m in plugins} - ids), 'logs': logs[-10:]}
                piano = next((i for i in snapshot['instances'] if i['plugin_id'] == 'org.rackforge.concert-grand'))
                initial_pools = pools()
                if args.require_parallel and phase != 'cold':
                    assert initial_pools and all(p['ready'] == (1 << p['workers']) - 1 for p in initial_pools), {'pools': initial_pools, 'logs': logs[-20:]}
                command({'type': 'set_play_chain', 'instrument_id': piano['instance_id'], 'effects': []})
                for plugin in sorted(plugins, key=lambda m: (m['kind'] == 'effect', m['name'])):
                    instance = next((i for i in snapshot['instances'] if i['plugin_id'] == plugin['id']))
                    if plugin['kind'] == 'effect':
                        command({'type': 'select_plugin', 'instance_id': piano['instance_id']})
                        command({'type': 'set_play_chain', 'instrument_id': piano['instance_id'], 'effects': [{'id': 'cache-test-fx', 'plugin_id': plugin['id'], 'enabled': True}]})
                    else:
                        command({'type': 'select_plugin', 'instance_id': instance['instance_id']})
                        frame = page.locator('.plugin-frame-stage iframe')
                        frame.wait_for(state='attached')
                        page.wait_for_function("Boolean(document.querySelector('.plugin-frame-stage iframe')?.classList.contains('loaded'))", polling=100)
                    page.wait_for_timeout(500)
                    before = pools()
                    peak, nonzero = audio()
                    after = pools()
                    advanced = [p for p in after if any(q['instance'] == p['instance'] and q['epoch'] == p['epoch'] and p['blocks'] > q['blocks'] for q in before)]
                    if args.require_parallel and phase != 'cold' and 'parallel_render_v1' in plugin.get('capabilities', []):
                        assert advanced, {'plugin': plugin['name'], 'before': before, 'after': after}
                    if args.require_parallel and phase != 'cold' and (plugin['id'] == 'org.rackforge.concert-grand' or plugin['kind'] == 'effect'):
                        assert any(p['geometry'].get('reportStride', 0) > 0 for p in advanced), {'plugin': plugin['name'], 'before': before, 'after': after, 'logs': logs[-20:]}
                    assert [(p['instance'],p['epoch']) for p in after] == [(p['instance'],p['epoch']) for p in initial_pools], 'Selecting an instrument replaced a live worker pool'
                    record = {'phase': phase, 'plugin': plugin['name'], 'version': plugin['version'], 'isolated': page.evaluate('crossOriginIsolated'), 'peak': peak, 'nonzero_blocks': nonzero, 'kind': plugin['kind'], 'parallel_pools': advanced}
                    results.append(record)
                    print('PLUGIN_AUDIO', json.dumps(record), flush=True)
                    args.output.write_text(json.dumps({'results': results, 'switches': switches, 'logs': logs}, indent=2))
                    assert peak > 1e-05 and nonzero > 0, record
                command({'type': 'set_play_chain', 'instrument_id': piano['instance_id'], 'effects': []})
                check_switches(snapshot, phase)
                if args.delay_worker_ms and phase != 'cold':
                    command({'type': 'select_plugin', 'instance_id': piano['instance_id']})
                    before = pools()
                    page.evaluate('ms => Atomics.store(window.testWorkerDelay,0,ms)', args.delay_worker_ms)
                    peak, nonzero = audio()
                    after = pools()
                    assert peak > 1e-5 and nonzero >= 3, {'phase':phase, 'peak':peak, 'nonzero':nonzero}
                    assert sum(p['deferred'] for p in after) > sum(p['deferred'] for p in before), {'before':before,'after':after}
                    recovery = {'phase':phase,'delayed_worker_ms':args.delay_worker_ms,'peak':peak,'nonzero_windows':nonzero,'pools':after}
                    switches.append(recovery)
                    print('DELAYED_WORKER_RECOVERED', json.dumps(recovery), flush=True)
                    args.output.write_text(json.dumps({'results':results,'switches':switches,'logs':logs},indent=2))
                page.wait_for_timeout(600)
            context.close()
            browser.close()
        print('ALL_PLUGINS_AUDIO_OK', len(plugins), len(results), flush=True)
    finally:
        server.shutdown()
        server.server_close()
if __name__ == '__main__':
    main()
