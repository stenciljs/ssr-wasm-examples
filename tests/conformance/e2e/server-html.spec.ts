/**
 * Checks the raw HTML each example returns, without running any JavaScript.
 * These are the guarantees server rendering exists for: content, structure and styles are there
 * before the client bundle loads.
 */
import { expect, test } from '@playwright/test';

/** Matches the attributes of an open tag, allowing `>` inside quoted values (valid HTML). */
const ATTRS = String.raw`(?:[^>"]|"[^"]*")*`;

const MODES = [
  { path: '/', name: 'middleware' },
  { path: '/helper', name: 'helper' },
] as const;

test('GET /health', async ({ request }) => {
  const res = await request.get('/health');
  expect(res.status()).toBe(200);
  expect((await res.text()).trim()).toBe('ok');
});

test('client bundle is served under /stencil/', async ({ request }) => {
  const res = await request.get('/stencil/demo-components.esm.js');
  expect(res.status()).toBe(200);
  expect(res.headers()['content-type']).toMatch(/javascript/);
});

for (const mode of MODES) {
  test.describe(`${mode.name} mode (${mode.path})`, () => {
    let html: string;

    test.beforeEach(async ({ request }) => {
      const res = await request.get(mode.path);
      expect(res.status()).toBe(200);
      expect(res.headers()['content-type']).toMatch(/text\/html/);
      html = await res.text();
    });

    test('every component was server-rendered', () => {
      for (const tag of ['demo-notice', 'demo-card', 'demo-badge', 'demo-counter', 'demo-task-list']) {
        expect(html, `<${tag}> should carry a hydration id`).toMatch(new RegExp(`<${tag}\\b${ATTRS}\\ss-id="\\d+"`));
      }
    });

    test('shadow components render as Declarative Shadow DOM with their styles', () => {
      for (const tag of ['demo-card', 'demo-counter', 'demo-task-list']) {
        expect(html).toMatch(
          new RegExp(`<${tag}\\b${ATTRS}>\\s*<template shadowrootmode="open"><style sty-id="sc-${tag}">`),
        );
      }
    });

    test('shadow component content is in the shadow root', () => {
      expect(html).toMatch(/<h2[^>]*>(<!--[^>]*-->)?Server-rendered card<\/h2>/);
      expect(html).toMatch(/<span class="value"[^>]*>(<!--[^>]*-->)?5<\/span>/);
      expect(html).toMatch(/<button[^>]*>(<!--[^>]*-->)?\+2<\/button>/);
    });

    test('light DOM / slotted content is preserved', () => {
      expect(html).toContain('<span slot="icon">★</span>');
      expect(html).toContain('This card was rendered on the server as Declarative Shadow DOM.');
      expect(html).toContain('<span slot="footer">Footer slot</span>');
    });

    test('unencapsulated component renders into the light DOM', () => {
      expect(html).toMatch(/<demo-notice[^>]*class="[^"]*demo-notice--info/);
      expect(html).toMatch(/<strong class="demo-notice__label"[^>]*>(<!--[^>]*-->)?Note:<\/strong>/);
      expect(html).toMatch(/Rendered by [^<]+\./);
    });

    test('scoped component carries scoped classes', () => {
      expect(html).toMatch(/<demo-badge[^>]*class="[^"]*sc-demo-badge-h/);
      expect(html).toMatch(/<span class="badge sc-demo-badge[^"]*"/);
    });

    test('scoped component styles are in the document (not just added by the client)', () => {
      // demo-badge in demo-card's light DOM needs `sc-demo-badge` styles outside any shadow root.
      // Inside demo-task-list's shadow root they're inlined into that root's <style>, which
      // doesn't help the light-DOM badge.
      const outsideShadowRoots = html.replace(/<template shadowrootmode="open">[\s\S]*?<\/template>/g, '');
      expect(outsideShadowRoots).toMatch(/<style sty-id="sc-demo-badge"/);
    });

    test('complex prop survives the template engine + JSON round trip', () => {
      expect(html).toMatch(/<p class="summary"[^>]*>(<!--[^>]*-->)?2(<!--[^>]*-->)? of (<!--[^>]*-->)?3(<!--[^>]*-->)? remaining/);
      expect(html).toContain('Write components');
      expect(html).toContain('Render with &lt;wasm&gt; &amp; "friends"');
      expect(html).toContain('Hydrate on the client');
      expect(html.match(/<demo-badge\b/g)?.length).toBe(4); // 1 in the card + 3 in the task list
    });

    test('hydration ids are unique within the page', () => {
      const ids = [...html.matchAll(/\ss-id="(\d+)"/g)].map((m) => m[1]);
      expect(ids.length).toBe(8);
      expect(new Set(ids).size).toBe(ids.length);
    });

    test('hydration ids start from 1 on every request', async ({ request }) => {
      for (let i = 0; i < 3; i++) {
        const again = await (await request.get(mode.path)).text();
        const ids = [...again.matchAll(/\ss-id="(\d+)"/g)].map((m) => Number(m[1]));
        expect(Math.min(...ids)).toBe(1);
      }
    });

    test('concurrent requests all render correctly', async ({ request }) => {
      const pages = await Promise.all(Array.from({ length: 16 }, () => request.get(mode.path)));
      for (const res of pages) {
        expect(res.status()).toBe(200);
        const body = await res.text();
        expect(body).toContain('<template shadowrootmode="open">');
        const ids = [...body.matchAll(/\ss-id="(\d+)"/g)].map((m) => Number(m[1]));
        expect(ids.sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
      }
    });
  });
}

test('helper mode leaves the layout alone', async ({ request }) => {
  const html = await (await request.get('/helper')).text();
  // whole-document rendering would add these to <html>
  expect(html).not.toMatch(/<html[^>]*data-stencil-build/);
  expect(html).not.toMatch(/<html[^>]*class="[^"]*hydrated/);
});

test('middleware mode renders the full document', async ({ request }) => {
  const html = await (await request.get('/')).text();
  expect(html).toMatch(/<html[^>]*data-stencil-build/);
  expect(html).toMatch(/<html[^>]*lang="en"/);
});
