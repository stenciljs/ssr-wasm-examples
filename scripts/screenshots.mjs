/**
 * Screenshot each running example's `/` page (after hydration, clipped to <main>) into
 * docs/screenshots/<example>.png. Used for the README gallery.
 *
 *   node scripts/screenshots.mjs php-laravel=8001 python-django=8002 ...
 *
 * Start the examples first (scripts/conformance-all.sh starts and stops them one at a time;
 * scripts/screenshots.sh wraps this file with the same start/stop).
 */
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// playwright is a dependency of tests/conformance
const require = createRequire(path.join(root, 'tests/conformance/package.json'));
const { chromium } = require('@playwright/test');

const outDir = path.join(root, 'docs/screenshots');
await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
try {
  for (const arg of process.argv.slice(2)) {
    const [name, port] = arg.split('=');
    const page = await browser.newPage({ viewport: { width: 760, height: 900 }, deviceScaleFactor: 2 });
    await page.goto(`http://localhost:${port}/`);
    // wait until the client runtime has hydrated every component
    await page.waitForFunction(() =>
      ['demo-notice', 'demo-card', 'demo-badge', 'demo-counter', 'demo-task-list'].every((t) =>
        [...document.querySelectorAll(t)].every((el) => typeof el.componentOnReady === 'function'),
      ),
    );
    await page.evaluate(() =>
      Promise.all([...document.querySelectorAll('[s-id], demo-notice, demo-card, demo-counter, demo-task-list, demo-badge')]
        .filter((el) => typeof el.componentOnReady === 'function')
        .map((el) => el.componentOnReady())),
    );
    const file = path.join(outDir, `${name}.png`);
    await page.locator('main').screenshot({ path: file });
    console.log(`${name}: ${path.relative(root, file)}`);
    await page.close();
  }
} finally {
  await browser.close();
}
