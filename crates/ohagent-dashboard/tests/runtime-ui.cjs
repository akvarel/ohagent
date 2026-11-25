const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const base = process.env.RUNTIME_UI_BASE_URL || 'http://127.0.0.1:9190';
(async () => {
  const dist = path.resolve(__dirname, '../../../target/dashboard-dist');
  const assets = fs.readdirSync(path.join(dist, 'assets'));
  assert.equal(assets.some(name => name.startsWith('Demo-')), false);
  const bundle = assets.filter(name => name.endsWith('.js')).map(name => fs.readFileSync(path.join(dist, 'assets', name), 'utf8')).join('\n');
  for (const copy of ['Karmina', 'Celotleti.lv', 'Medicine Reminder', 'Specialist agents.', 'A focused product ecosystem.']) assert.equal(bundle.includes(copy), false, copy);
  const browser = await chromium.launch({ headless: true });
  let checks = 0;
  try {
    for (const width of [390, 1440]) {
      const context = await browser.newContext({ viewport: { width, height: 1000 } });
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      // This isolates UI mounting from unavailable authenticated operator APIs.
      await page.route(base + '/api/**', route => route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"test backend unavailable"}' }));
      await page.goto(base + '/');
      await page.waitForURL('**/operator');
      for (const language of ['en', 'lv', 'ru']) {
        await page.locator('select').selectOption(language);
        await page.waitForFunction(lang => document.documentElement.lang === lang, language);
        await page.reload();
        await page.waitForFunction(lang => document.documentElement.lang === lang, language);
        for (const route of ['/operator', '/progress', '/skills', '/memory', '/keys', '/usage', '/agent-as-a-service/app/processes']) {
          await page.goto(base + route);
          await page.locator('main').waitFor();
          assert.equal(await page.locator('meta[name="robots"]').getAttribute('content'), 'noindex, nofollow');
          assert.equal(await page.locator('.oh-public, .oh-product-card').count(), 0);
          if (route.includes('/app/')) {
            await page.locator('.aaas-brand').waitFor();
            assert.equal(await page.locator('.aaas-brand').getAttribute('href'), '/agent-as-a-service/app');
          }
          checks++;
        }
        await page.goto(base + '/operator');
      }
      assert.deepEqual(errors, []);
      await context.close();
    }
  } finally { await browser.close(); }
  console.log(`PASS: ${checks} runtime routes/locales/viewports, language persistence, root redirect and no corporate bundle.`);
})().catch(error => { console.error(error); process.exitCode = 1; });
