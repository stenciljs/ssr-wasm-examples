/**
 * Reference implementation of CONTRACT.md, in plain Node.
 *
 * It isn't an example (Node apps should use the regular `ssr` output target). It exists so
 * the conformance suite can be validated against a known-good server before it's pointed at
 * the language examples.
 *
 *   node reference-server.mjs   # http://localhost:8000
 */
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import createPlugin from '@extism/extism';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const WASM = process.env.SSR_WASM_PATH ?? path.join(root, 'packages/components/dist/ssr-wasm/index.wasm');
const CLIENT_DIR =
  process.env.STENCIL_CLIENT_DIR ?? path.join(root, 'packages/components/dist/loader-bundle/demo-components');
const PORT = Number(process.env.PORT ?? 8000);
const FRAMEWORK = 'Node reference';

// ---- plugin: one instance, calls serialized (an instance isn't safe for concurrent calls) ----

const plugin = await createPlugin(WASM, { useWasi: true });
let queue = Promise.resolve();
/** Run `fn` with exclusive use of the plugin for the duration of one request. */
const withPlugin = (fn) => {
  const run = queue.then(() => fn(ssr));
  queue = run.catch(() => {});
  return run;
};
const ssr = {
  async reset() {
    await plugin.call('resetSsrDocData', '');
  },
  async render(html, options = {}) {
    const out = await plugin.call('renderToString', JSON.stringify({ html, options }));
    const result = JSON.parse(out.text());
    if (result.__error) {
      console.error('[ssr-wasm]', result.__error);
      return html;
    }
    return result.html;
  },
};

// ---- page ----

const TASKS = [
  { title: 'Write components', done: true },
  { title: 'Render with <wasm> & "friends"', done: false },
  { title: 'Hydrate on the client' },
];

const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

const components = {
  notice: () => `<demo-notice kind="info">Rendered by ${esc(FRAMEWORK)}.</demo-notice>`,
  card: () => `<demo-card heading="Server-rendered card" variant="highlight">
        <span slot="icon">★</span>
        <p>This card was rendered on the server as Declarative Shadow DOM.</p>
        <demo-badge tone="success">SSR</demo-badge>
        <span slot="footer">Footer slot</span>
      </demo-card>`,
  counter: () => `<demo-counter start="5" step="2"></demo-counter>`,
  tasks: () => `<demo-task-list tasks="${esc(JSON.stringify(TASKS))}"></demo-task-list>`,
};

const layout = (c) => `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>${esc(FRAMEWORK)} · Stencil SSR WASM</title>
    <style>
      body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 2rem auto; }
      main > * { margin-bottom: 1.5rem; }
      .demo-notice { display: block; padding: .75rem 1rem; border-radius: 6px; background: #eef2ff; }
      .demo-notice--error { background: #fee2e2; }
    </style>
    <script type="module" src="/stencil/demo-components.esm.js"></script>
  </head>
  <body>
    <main>
      <h1>${esc(FRAMEWORK)} + Stencil SSR WASM</h1>

      ${c.notice}

      ${c.card}

      ${c.counter}

      ${c.tasks}
    </main>
  </body>
</html>
`;

// ---- routes ----

const MIME = { '.js': 'text/javascript', '.css': 'text/css', '.map': 'application/json' };

createServer(async (req, res) => {
  const url = new URL(req.url, 'http://x');
  try {
    if (url.pathname === '/health') {
      res.writeHead(200, { 'content-type': 'text/plain' }).end('ok');
    } else if (url.pathname === '/') {
      // middleware mode: render the page, then post-process the whole document
      const page = layout(Object.fromEntries(Object.entries(components).map(([k, f]) => [k, f()])));
      // full-document renders number their hydration ids from 1 by themselves: no reset needed
      const html = await withPlugin((s) => s.render(page));
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(html);
    } else if (url.pathname === '/helper') {
      // helper mode: each component rendered as a fragment, the layout is left alone
      const html = await withPlugin(async (s) => {
        // fragments share one id sequence: reset once, before the first fragment, so the same
        // page renders the same way every time - never between fragments, or their ids collide
        await s.reset();
        const c = {};
        for (const [k, f] of Object.entries(components)) c[k] = await s.render(f(), { fullDocument: false });
        return layout(c);
      });
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(html);
    } else if (url.pathname.startsWith('/stencil/')) {
      const file = path.join(CLIENT_DIR, path.normalize(url.pathname.slice('/stencil/'.length)));
      if (!file.startsWith(CLIENT_DIR)) throw Object.assign(new Error('bad path'), { code: 'ENOENT' });
      const body = await readFile(file);
      res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream' }).end(body);
    } else {
      res.writeHead(404).end('not found');
    }
  } catch (e) {
    if (e.code === 'ENOENT') return res.writeHead(404).end('not found');
    console.error(e);
    res.writeHead(500).end('error');
  }
}).listen(PORT, () => console.log(`reference server on http://localhost:${PORT}`));
