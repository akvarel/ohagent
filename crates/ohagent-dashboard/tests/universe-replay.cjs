// Uses a recorded actual-runtime journal. No auth mock, API mutation or inference.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const evidenceDir = path.resolve(__dirname, '../../../docs/evidence/aaas-001');
const trace = JSON.parse(fs.readFileSync(path.join(evidenceDir, 'live-runtime.json'))).runs[0];
const snapshots = trace.events.map(event => event.payload.snapshot).filter(Boolean);
(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const width of [1440, 390]) {
      const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: 'reduce', ...(width === 1440 ? { recordVideo: { dir: evidenceDir, size: { width: 1440, height: 1000 } } } : {}) });
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.goto((process.env.PUBLIC_UI_BASE_URL || 'http://127.0.0.1:5184') + '/tests/universe-replay.html');
      await page.locator('[data-testid=position]').waitFor();
      for (let index = 0; index < snapshots.length; index++) {
        await page.getByRole('slider', { name: 'Stored event' }).fill(String(index));
        await page.waitForFunction(expected => document.querySelector('[data-testid=position]').textContent.startsWith(expected + '/'), index + 1);
        assert.equal(await page.locator('.aaas-node-running').count(), snapshots[index].tasks.filter(task => task.status === 'running').length);
        assert.equal(await page.locator('.aaas-node').count(), 4);
        await page.waitForTimeout(width === 1440 ? 140 : 0);
      }
      for (const language of ['en', 'lv', 'ru']) {
        await page.getByRole('combobox').selectOption(language);
        await page.locator('.aaas-stages button').first().click();
        assert.ok((await page.locator('.aaas-inspector').innerText()).includes(trace.run.tasks[0].agent_instance_id));
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, 'no horizontal overflow');
      }
      await page.screenshot({ path: path.join(evidenceDir, `universe-recorded-${width}.png`), fullPage: true });
      assert.deepEqual(errors, []);
      const video = page.video();
      await context.close();
      if (video) { await video.saveAs(path.join(evidenceDir, 'universe-recorded.webm')); await video.delete(); }
    }
    console.log('PASS: real-trace component snapshots, four actual instances, parallel states, EN/LV/RU, desktop/mobile and recording. Not live OIDC E2E.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
