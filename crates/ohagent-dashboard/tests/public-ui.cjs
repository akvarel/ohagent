/* Run against a local Vite server. Requires Playwright in normal Node resolution
 * or NODE_PATH. Does not install browsers, call external services or mutate data.
 * PUBLIC_UI_BASE_URL defaults to http://127.0.0.1:5184.
 */
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const base = process.env.PUBLIC_UI_BASE_URL || 'http://127.0.0.1:5184';
const browserOptions = { headless: true };
if (process.env.PLAYWRIGHT_EXECUTABLE_PATH) browserOptions.executablePath = process.env.PLAYWRIGHT_EXECUTABLE_PATH;
if (process.env.PLAYWRIGHT_HOST_RESOLVER_RULES) browserOptions.args = [`--host-resolver-rules=${process.env.PLAYWRIGHT_HOST_RESOLVER_RULES}`];

(async () => {
  const browser = await chromium.launch(browserOptions);
  const failures = [];
  let checks = 0;
  try {
    for (const width of [390, 1440]) {
      const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: 'reduce', locale: 'en-US' });
      const page = await context.newPage();
      const errors = [];
      const requests = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => requests.push(request.url()));
      for (const language of ['en', 'lv', 'ru']) {
        await page.goto(base + '/');
        if (language === 'en') {
          assert.equal(requests.some(url => /\/(?:Demo|AaasApp)[-.]/.test(new URL(url).pathname)), false, 'Demo and product app modules must not load on the landing page');
          checks++;
        }
        await page.locator('.oh-language select').selectOption(language);
        await page.waitForFunction(language => document.documentElement.lang === language, language);
        const expected = { en: 'Specialist agents.', lv: 'Specializēti aģenti.', ru: 'Специализированные агенты.' }[language];
        assert.ok((await page.locator('h1').textContent()).includes(expected));
        assert.equal(await page.evaluate(() => localStorage.getItem('ohagent-language')), language);
        await page.reload();
        await page.waitForFunction(language => document.documentElement.lang === language, language);
        assert.ok((await page.locator('h1').textContent()).includes(expected));
        for (const route of ['/', '/agent-as-a-service', '/agent-as-a-service/demo']) {
          await page.goto(base + route);
          await page.locator('h1').waitFor();
          assert.equal(await page.locator('h1').count(), 1);
          assert.equal(await page.locator('meta[name="robots"]').getAttribute('content'), route.endsWith('/demo') ? 'noindex, nofollow' : 'index, follow');
          assert.ok((await page.locator('meta[name="description"]').getAttribute('content')).length > 30);
          assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, `${language} ${width} ${route} has horizontal overflow`);
          if (route === '/') {
            assert.equal(await page.locator('.oh-product-card').count(), 3);
            assert.ok(await page.locator('a[href="https://bugzero.lv/"]').count());
            assert.ok(await page.locator('a[href="https://github.com/Graphify-Labs/graphify"]').count());
            assert.equal(await page.locator('a[href^="mailto:"]').count(), 0);
          }
          if (route.endsWith('/demo')) {
            const run = page.locator('.oh-demo-definition .oh-button');
            for (let i = 0; i < 3; i++) await run.click();
            assert.equal(await run.isDisabled(), true);
            assert.equal(await page.locator('.oh-demo-stages .is-complete').count(), 3);
            assert.ok((await page.locator('[role="status"] p').textContent()).length > 30);
            await page.locator('.oh-reset').click();
            assert.equal(await page.locator('.oh-demo-stages .is-complete').count(), 0);
            assert.equal(await run.isEnabled(), true);
          }
          checks++;
        }
      }
      assert.equal(requests.filter(url => new URL(url).pathname.startsWith('/api')).length, 0, 'Public pages must not fetch operator or customer APIs');
      await page.goto(base + '/');
      await page.locator('.oh-language select').selectOption('en');
      await page.keyboard.press('Tab');
      // Start from a fresh document so the first tab is the skip link.
      await page.reload();
      await page.locator('h1').waitFor();
      await page.keyboard.press('Tab');
      assert.equal(await page.evaluate(() => document.activeElement.className), 'oh-skip');
      assert.equal(await page.locator('.oh-skip').evaluate(node => getComputedStyle(node).outlineStyle), 'solid');
      await page.keyboard.press('Enter');
      assert.equal(await page.evaluate(() => document.activeElement.id), 'main-content');
      assert.equal(await page.locator('.oh-nav a').first().evaluate(node => getComputedStyle(node).transitionDuration), '0s');
      checks++;
      await page.evaluate(() => { localStorage.setItem('ohagent-language', 'lv'); window.dispatchEvent(new CustomEvent('ohagent-language')); });
      await page.waitForFunction(() => document.documentElement.lang === 'lv');
      await page.evaluate(() => window.dispatchEvent(new CustomEvent('ohagent-language', { detail: 'ru' })));
      await page.waitForFunction(() => document.documentElement.lang === 'ru');
      checks++;
      // Operator checks explicitly simulate an unavailable backend. They prove
      // route preservation and graceful mounting, not authenticated API success.
      await page.route(base + '/api/**', route => route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"test backend unavailable"}' }));
      for (const route of ['/operator', '/progress', '/skills', '/memory', '/keys', '/usage']) {
        await page.goto(base + route);
        assert.equal(await page.locator('meta[name="robots"]').getAttribute('content'), 'noindex, nofollow');
        assert.ok((await page.locator('main').textContent()).length > 5);
        assert.equal(await page.locator('.oh-public h1').count(), 0, `Operator route ${route} must not fall through to public 404`);
        checks++;
      }
      await page.goto(base + '/agent-as-a-service/app/processes');
      await page.waitForFunction(() => document.querySelector('meta[name="robots"]')?.content === 'noindex, nofollow');
      assert.ok((await page.locator('body').textContent()).length > 5);
      checks++;
      const missing = await page.goto(base + '/missing-page');
      assert.ok(missing, 'missing route must produce a document response');
      if (missing.status() === 404) {
        // The compiled same-origin server deliberately refuses unknown routes.
        assert.equal(missing.headers()['x-content-type-options'], 'nosniff');
      } else {
        // Vite's development fallback loads the client-side localized 404.
        assert.equal(missing.status(), 200);
        await page.locator('.oh-public h1').waitFor();
        assert.ok((await page.locator('.oh-eyebrow').textContent()).includes('404'));
        assert.equal(await page.locator('meta[name="robots"]').getAttribute('content'), 'noindex, nofollow');
      }
      checks++;
      if (process.env.PUBLIC_UI_ARTIFACTS) {
        await page.goto(base + '/');
        await page.locator('.oh-language select').selectOption('en');
        await page.screenshot({ path: `${process.env.PUBLIC_UI_ARTIFACTS}/home-${width}.png`, fullPage: true });
        await page.goto(base + '/agent-as-a-service');
        await page.screenshot({ path: `${process.env.PUBLIC_UI_ARTIFACTS}/product-${width}.png`, fullPage: true });
      }
      assert.deepEqual(errors, [], `Browser exceptions at ${width}px`);
      await context.close();
    }
    // Storage can be blocked by browser policy. Language changes must remain
    // usable in memory instead of throwing or silently reverting.
    const blocked = await browser.newContext({ locale: 'en-US' });
    await blocked.addInitScript(() => { Object.defineProperty(Storage.prototype, 'getItem', { value: () => { throw new DOMException('blocked'); } }); Object.defineProperty(Storage.prototype, 'setItem', { value: () => { throw new DOMException('blocked'); } }); });
    const page = await blocked.newPage();
    await page.goto(base + '/');
    await page.locator('.oh-language select').selectOption('lv');
    await page.waitForFunction(() => document.documentElement.lang === 'lv');
    checks++;
    await blocked.close();
    console.log(`PASS: ${checks} browser route/viewport/locale scenarios, keyboard, reduced motion, persistence, sample reset, no public API calls and blocked storage.`);
  } catch (error) { failures.push(error); }
  finally { await browser.close(); }
  if (failures.length) { for (const failure of failures) console.error(failure); process.exitCode = 1; }
})();
