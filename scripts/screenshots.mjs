/**
 * Screenshot each running example's `/` page into docs/screenshots/<example>.png, for the README.
 *
 * JavaScript is disabled in the browser, so each screenshot is only the server's HTML: shadow
 * roots come from Declarative Shadow DOM, and no client-side code renders or hydrates anything.
 *
 *   node scripts/screenshots.mjs php-laravel=8001 python-django=8002 ...
 *
 * Start the examples first (each on its port from CONTRACT.md).
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
    const context = await browser.newContext({
      javaScriptEnabled: false,
      viewport: { width: 760, height: 900 },
      deviceScaleFactor: 2,
    });
    const page = await context.newPage();
    await page.goto(`http://localhost:${port}/`, { waitUntil: 'load' });

    // Don't publish a screenshot of a broken render. Locators look inside open shadow roots, so
    // these only match if the browser attached the server's Declarative Shadow DOM.
    const badges = await page.locator('demo-task-list demo-badge').allTextContents();
    if (badges.join() !== 'done,todo,todo') throw new Error(`${name}: task badges rendered as ${JSON.stringify(badges)}`);
    const heading = await page.locator('demo-card h2').textContent();
    if (heading !== 'Server-rendered card') throw new Error(`${name}: card heading is ${JSON.stringify(heading)}`);

    const file = path.join(outDir, `${name}.png`);
    await page.locator('main').screenshot({ path: file });
    console.log(`${name}: ${path.relative(root, file)}`);
    await context.close();
  }
} finally {
  await browser.close();
}
