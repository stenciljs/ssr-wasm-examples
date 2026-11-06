/**
 * Calls the ssr-wasm plugin directly (no server), to pin down the plugin behaviour the
 * examples and CONTRACT.md rely on.
 */
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import createPlugin from '@extism/extism';
import { beforeAll, describe, expect, it } from 'vitest';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const WASM = process.env.SSR_WASM_PATH ?? path.join(root, 'packages/components/dist/ssr-wasm/index.wasm');

let plugin: Awaited<ReturnType<typeof createPlugin>>;

const render = async (html: string, options: Record<string, unknown> = {}) => {
  const out = await plugin.call('renderToString', JSON.stringify({ html, options }));
  return JSON.parse(out!.text());
};
const reset = () => plugin.call('resetSsrDocData', '');
const ids = (html: string) => [...html.matchAll(/\ss-id="(\d+)"/g)].map((m) => Number(m[1]));

beforeAll(async () => {
  plugin = await createPlugin(WASM, { useWasi: true });
});

describe('renderToString', () => {
  it('returns render results as JSON', async () => {
    const result = await render('<demo-counter start="3"></demo-counter>');
    expect(result.__error).toBeUndefined();
    expect(result.hydratedCount).toBe(1);
    expect(result.diagnostics).toEqual([]);
    expect(result.html).toContain('<template shadowrootmode="open">');
  });

  it('parses a JSON attribute for a complex prop', async () => {
    const tasks = JSON.stringify([{ title: 'a', done: true }, { title: 'b' }]).replace(/"/g, '&quot;');
    const { html } = await render(`<demo-task-list tasks="${tasks}"></demo-task-list>`, { fullDocument: false });
    expect(html).toMatch(/1(<!--[^>]*-->)? of (<!--[^>]*-->)?2(<!--[^>]*-->)? remaining/);
  });
});

describe('hydration ids', () => {
  it('full-document renders always start from 1, without a reset', async () => {
    const page = '<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>';
    for (let i = 0; i < 3; i++) {
      expect(ids((await render(page)).html)).toEqual([1, 2]);
    }
  });

  it('fragment renders keep counting across calls until resetSsrDocData', async () => {
    await reset();
    const first = ids((await render('<demo-badge></demo-badge>', { fullDocument: false })).html);
    const second = ids((await render('<demo-badge></demo-badge>', { fullDocument: false })).html);
    expect(first).toEqual([1]);
    expect(second).toEqual([2]);

    await reset();
    expect(ids((await render('<demo-badge></demo-badge>', { fullDocument: false })).html)).toEqual([1]);
  });

  it('a full-document render does not disturb the fragment sequence', async () => {
    await reset();
    expect(ids((await render('<demo-badge></demo-badge>', { fullDocument: false })).html)).toEqual([1]);
    await render('<html><body><demo-badge></demo-badge></body></html>');
    expect(ids((await render('<demo-badge></demo-badge>', { fullDocument: false })).html)).toEqual([2]);
  });

  it('full documents give identical output for identical input (apart from the random build id)', async () => {
    const page = '<html><body><demo-card heading="x"><demo-badge>y</demo-badge></demo-card></body></html>';
    const a = (await render(page, { buildId: 'fixed' })).html;
    const b = (await render(page, { buildId: 'fixed' })).html;
    expect(a).toBe(b);
  });
});

describe('fragments (fullDocument: false)', () => {
  it('carry the styles of scoped components', async () => {
    const { html } = await render('<demo-badge tone="success">ok</demo-badge>', { fullDocument: false });
    expect(html).toMatch(/^<style sty-id="sc-demo-badge">/);
  });

  it('carry the styles of shadow components rendered as scoped', async () => {
    const { html } = await render('<demo-counter></demo-counter>', {
      fullDocument: false,
      serializeShadowRoot: 'scoped',
    });
    expect(html).toMatch(/^<style sty-id="sc-demo-counter">/);
    expect(html).not.toContain('<template shadowrootmode');
  });

  it("don't add styles for shadow components (they're inside the DSD template)", async () => {
    const { html } = await render('<demo-counter></demo-counter>', { fullDocument: false });
    expect(html).toMatch(/^<demo-counter/);
  });
});
