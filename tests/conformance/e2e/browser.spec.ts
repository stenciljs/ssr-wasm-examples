/**
 * Loads each page in a real browser. Checks it looks right with JavaScript off (pure SSR output),
 * then that the client bundle hydrates the server markup instead of re-rendering or
 * duplicating it.
 */
import { expect, test, type Page } from '@playwright/test';

const MODES = [
  { path: '/', name: 'middleware' },
  { path: '/helper', name: 'helper' },
] as const;

/** Collect anything that looks like a problem while the page loads and hydrates. */
function watchForErrors(page: Page) {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error' || (m.type() === 'warning' && /stencil|hydrat/i.test(m.text()))) {
      errors.push(`console.${m.type()}: ${m.text()}`);
    }
  });
  page.on('response', (r) => {
    if (r.status() >= 400) errors.push(`HTTP ${r.status()} ${r.url()}`);
  });
  return errors;
}

const TAGS = ['demo-notice', 'demo-card', 'demo-badge', 'demo-counter', 'demo-task-list'];

/**
 * Wait until the client runtime has taken over every component.
 *
 * Don't use the `hydrated` class for this: server-rendered markup already carries it (so hosts
 * aren't hidden before the client loads), so it's there before any JavaScript runs.
 * `componentOnReady()` only exists once the client has upgraded the element, and resolves
 * after it has hydrated.
 */
async function waitForClientHydration(page: Page) {
  await page.waitForFunction(
    (tags) =>
      tags.every((t) =>
        [...document.querySelectorAll(t)].every((el) => typeof (el as any).componentOnReady === 'function'),
      ),
    TAGS,
  );
  await page.evaluate(
    (tags) =>
      Promise.all(tags.flatMap((t) => [...document.querySelectorAll(t)].map((el) => (el as any).componentOnReady()))),
    TAGS,
  );
}

for (const mode of MODES) {
  test.describe(`${mode.name} mode (${mode.path})`, () => {
    test.describe('without JavaScript', () => {
      test.use({ javaScriptEnabled: false });

      test('shadow roots are attached from Declarative Shadow DOM', async ({ page }) => {
        await page.goto(mode.path);
        // Playwright's locators pierce open shadow roots, so these only match if the browser
        // attached the declarative shadow roots.
        await expect(page.locator('demo-card h2')).toHaveText('Server-rendered card');
        await expect(page.locator('demo-counter .value')).toHaveText('5');
        await expect(page.locator('demo-task-list li')).toHaveCount(3);
      });

      test('components are styled before any JS runs', async ({ page }) => {
        await page.goto(mode.path);
        // demo-card (shadow): accent border from its shadow styles + the `highlight` variant
        await expect(page.locator('demo-card .card')).toHaveCSS('border-left-color', 'rgb(255, 107, 53)');
        // demo-counter (shadow)
        await expect(page.locator('demo-counter button')).toHaveCSS('background-color', 'rgb(88, 81, 255)');
        // demo-badge (scoped) slotted into the card - needs scoped styles in the document
        await expect(page.locator('demo-card demo-badge .badge')).toHaveCSS('background-color', 'rgb(209, 250, 229)');
        // demo-badge (scoped) nested inside demo-task-list's shadow root
        await expect(page.locator('demo-task-list demo-badge .badge').first()).toHaveCSS(
          'background-color',
          'rgb(209, 250, 229)',
        );
      });
    });

    test.describe('with JavaScript', () => {
      test('hydrates without errors', async ({ page }) => {
        const errors = watchForErrors(page);
        await page.goto(mode.path);
        await waitForClientHydration(page);
        await page.waitForLoadState('networkidle');
        expect(errors).toEqual([]);
      });

      test('hydration reuses the server markup instead of duplicating it', async ({ page }) => {
        await page.goto(mode.path);
        await waitForClientHydration(page);
        const counts = await page.evaluate(() => {
          const card = document.querySelector('demo-card')!;
          const tasks = document.querySelector('demo-task-list')!;
          const notice = document.querySelector('demo-notice')!;
          return {
            cardArticles: card.shadowRoot!.querySelectorAll('article').length,
            cardBadges: card.querySelectorAll('demo-badge').length,
            badgeSpans: card.querySelector('demo-badge')!.querySelectorAll('.badge').length,
            taskItems: tasks.shadowRoot!.querySelectorAll('li').length,
            noticeLabels: notice.querySelectorAll('.demo-notice__label').length,
            noticeLabel: notice.querySelector('.demo-notice__label')!.textContent,
            // Stencil patches textContent on slotted non-shadow components to return only the
            // slotted (author) content, like shadow DOM would
            noticeText: notice.textContent!.replace(/\s+/g, ' ').trim(),
          };
        });
        expect(counts).toEqual({
          cardArticles: 1,
          cardBadges: 1,
          badgeSpans: 1,
          taskItems: 3,
          noticeLabels: 1,
          noticeLabel: 'Note:',
          noticeText: expect.stringMatching(/^Rendered by .+\.$/),
        });
      });

      test('hydrated components are interactive', async ({ page }) => {
        await page.goto(mode.path);
        const counter = page.locator('demo-counter');
        await waitForClientHydration(page);
        await expect(counter.locator('.value')).toHaveText('5');
        await counter.locator('button').click();
        await expect(counter.locator('.value')).toHaveText('7');
        await counter.locator('button').click();
        await expect(counter.locator('.value')).toHaveText('9');
      });

      test('complex prop is still correct after hydration', async ({ page }) => {
        await page.goto(mode.path);
        await waitForClientHydration(page);
        await expect(page.locator('demo-task-list .summary')).toHaveText('2 of 3 remaining');
        await expect(page.locator('demo-task-list li .title').nth(1)).toHaveText('Render with <wasm> & "friends"');
      });
    });
  });
}
