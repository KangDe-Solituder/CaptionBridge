import test from 'node:test';
import assert from 'node:assert/strict';
import { translationRoutes, recognitionChoices } from '../src/lib/model-choices.ts';
const make = () => ({
  llm: { profile_id: 'b' },
  llm_profiles: ['a', 'b', 'c'].map(id => ({ id, api_key_configured: true, config: { endpoint: 'https://api.deepseek.com/v1' } })),
  captions: { cloud: { openai: { endpoint: 'https://api.openai.com/v1/audio/transcriptions' }, deepseek: { profile_id: 'b', endpoint: 'https://api.deepseek.com/v1/tools/asr' } } },
  ocr: { cloud_profile_id: 'b', cloud_endpoint: 'https://api.deepseek.com/v1/tools/ocr' },
});
test('migration preserves active model; deletion repairs only invalid routes', () => {
  const s = make();
  assert.deepEqual(translationRoutes(s), { selection: 'b', live_caption: 'b', screenshot: 'b' });
  s.translation_profiles = { selection: 'a', live_caption: 'b', screenshot: 'a' };
  s.llm_profiles.pop();
  assert.deepEqual(translationRoutes(s), s.translation_profiles);
  s.llm_profiles = s.llm_profiles.filter(p => p.id !== 'b');
  assert.deepEqual(translationRoutes(s), { selection: 'a', live_caption: 'a', screenshot: 'a' });
  s.llm_profiles = [];
  assert.deepEqual(translationRoutes(s), { selection: null, live_caption: null, screenshot: null });
});
test('missing shared credentials exclude both cloud recognizers; installed local models remain', () => {
  const s = make();
  const models = [{ id: 'ready', kind: 'asr', status: 'available' }, { id: 'missing', kind: 'asr', status: 'not_installed' }, { id: 'ocr', kind: 'ocr', status: 'available' }];
  assert.deepEqual(recognitionChoices(s, models, 'asr').map(c => c.value), ['windows', 'local:ready', 'cloud:deepseek']);
  assert.deepEqual(recognitionChoices(s, models, 'ocr').map(c => c.value), ['local', 'deepseek']);
  s.llm_profiles = s.llm_profiles.filter(p => p.id !== 'b');
  assert.deepEqual(recognitionChoices(s, models, 'asr').map(c => c.value), ['windows', 'local:ready']);
  assert.deepEqual(recognitionChoices(s, models, 'ocr').map(c => c.value), ['local']);
});
test('cloud choices reject credentials sent to another host and missing keys', () => {
  const s = make();
  s.ocr.cloud_endpoint = 'https://example.com/v1/tools/ocr';
  assert.equal(recognitionChoices(s, [], 'ocr').length, 0);
  s.llm_profiles[1].api_key_configured = false;
  assert.deepEqual(recognitionChoices(s, [], 'asr').map(c => c.value), ['windows']);
});
