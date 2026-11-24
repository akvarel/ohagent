// Ten-minute synthetic renderer measurement. Does not certify durable-to-UI lag.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { spawn } = require('node:child_process');
const { chromium } = require('playwright');
(async () => {
  const server = spawn(process.execPath, [path.resolve(__dirname, '../node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '5185', '--strictPort'], { cwd: path.resolve(__dirname, '..'), stdio: 'ignore' });
  let browser;
  try {
    for (let n=0;n<100;n++) { try { if ((await fetch('http://127.0.0.1:5185')).ok) break; } catch {} await new Promise(resolve=>setTimeout(resolve,100)); }
    browser = await chromium.launch({ headless:true });
    const page = await browser.newPage({ viewport:{width:1440,height:1000}, reducedMotion:'reduce' });
    const errors=[];
    page.on('pageerror',error=>errors.push(error.message));
    await page.goto('http://127.0.0.1:5185/tests/universe-stress.html');
    await page.locator('output[data-sequence]').waitFor();
    const cdp=await page.context().newCDPSession(page);
    await cdp.send('HeapProfiler.collectGarbage');
    const before=await cdp.send('Runtime.getHeapUsage');
    const nodesBefore=await page.locator('*').count();
    console.log('JCODE_PROGRESS '+JSON.stringify({phase:'renderer stress',nodes:100,edges:200,rate:20,duration_seconds:600}));
    const started=Date.now();
    const samples=await page.evaluate(async()=>{
      await new Promise(resolve=>{let sequence=0;const timer=setInterval(()=>{window.fixture.send(++sequence);if(sequence===12000){clearInterval(timer);setTimeout(resolve,100);}},50)});
      return window.fixture.samples;
    });
    const interactions=[];
    for(let n=0;n<10;n++) {const start=performance.now();await page.locator('.aaas-stages button').nth(n).click();await page.getByRole('button',{name:'Zoom in',exact:true}).click();interactions.push(performance.now()-start);}
    await cdp.send('HeapProfiler.collectGarbage');
    const after=await cdp.send('Runtime.getHeapUsage');
    const nodesAfter=await page.locator('*').count();
    const percentile=(values,p)=>values.toSorted((a,b)=>a-b)[Math.min(values.length-1,Math.floor(values.length*p))];
    const result={kind:'synthetic-renderer-only-not-staging',executed_at:new Date().toISOString(),browser:browser.version(),cpu:os.cpus()[0].model,logical_cpus:os.cpus().length,host_memory_bytes:os.totalmem(),backend:'none, fixture only',viewport:'1440x1000',nodes:100,edges:200,workspace_run_summaries:10,rendered_graphs:1,event_rate:20,injected_events:12000,elapsed_ms:Date.now()-started,samples:samples.length,injection_to_next_frame_p95_ms:percentile(samples,.95),interaction_pair_p95_ms:percentile(interactions,.95),heap_before_bytes:before.usedSize,heap_after_bytes:after.usedSize,heap_growth_bytes:after.usedSize-before.usedSize,dom_before:nodesBefore,dom_after:nodesAfter,errors};
    fs.writeFileSync(path.resolve(__dirname,'../../../docs/evidence/aaas-001/renderer-stress.json'),JSON.stringify(result,null,2)+'\n');
    console.log(JSON.stringify(result));
    assert.deepEqual(errors,[]);
    assert.ok(result.elapsed_ms>=600000);
    assert.ok(result.samples>=11500,'unexpected rendering sample loss');
    assert.ok(result.injection_to_next_frame_p95_ms<2000,'fixture render latency exceeded target');
    assert.ok(result.heap_growth_bytes<20*1024*1024,'retained heap growth exceeded bounded fixture allowance');
    assert.ok(nodesAfter<=nodesBefore+30,'unexpected DOM accumulation');
  } finally { if(browser) await browser.close();server.kill('SIGTERM'); }
})().catch(error=>{console.error(error);process.exitCode=1;});
