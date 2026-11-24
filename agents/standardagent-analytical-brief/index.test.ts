import test from 'node:test';
import assert from 'node:assert/strict';
import { agents, prompts, models, __meta, exportTextProfile } from './index.ts';

test('portable pilot definitions distinguish agents from invocation instances', async () => {
  assert.deepEqual(__meta.entryAgents, ['coordinator', 'analyst', 'reviewer']);
  for (const name of __meta.entryAgents) {
    const agent = await agents[name]();
    assert.equal(agent.name, name);
    assert.equal(agent.sideB.human, true);
    assert.equal(agent.sideA.maxSteps, 1);
    const prompt = await prompts[agent.sideA.prompt]();
    assert.deepEqual(prompt.tools, []);
    const model = await models[prompt.model]();
    assert.equal(typeof model.provider, 'function');
    assert.equal(model.model, 'deepseek-v4-flash');
  }
});

test('text-only adapter exports exact declared semantics rather than arbitrary functions', async () => {
  const profile = await exportTextProfile();
  assert.equal(profile.specification_version, '0.2.0');
  assert.equal(profile.package_id, __meta.packageId);
  assert.equal(profile.agents.length, 3);
  assert.ok(profile.agents.every(a => a.model === 'deepseek:deepseek-v4-flash'));
  assert.ok(profile.agents.every(a => a.prompt.length > 80));
});
