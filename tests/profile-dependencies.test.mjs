import test from 'node:test';
import assert from 'node:assert/strict';
import { profileDependents } from '../src/lib/profile-dependencies.ts';

test('API dependencies and translation model dependencies remain separate', () => {
  const s = {
    captions: { cloud: { openai: {}, deepseek: { profile_id: 'shared' } } },
    ocr: { cloud_profile_id: 'shared' },
    translation_profiles: { selection: 'shared', live_caption: 'other', screenshot: 'shared' },
  };
  assert.deepEqual(profileDependents(s, 'shared'), {
    api: ['DeepSeek ASR', 'DeepSeek OCR'], model: ['划词翻译', '截图翻译'],
  });
  s.captions.cloud.deepseek.profile_id = null;
  s.ocr.cloud_profile_id = null;
  assert.deepEqual(profileDependents(s, 'shared'), { api: [], model: ['划词翻译', '截图翻译'] });
  assert.deepEqual(profileDependents(s, 'unused'), { api: [], model: [] });
});
