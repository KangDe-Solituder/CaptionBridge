import test from 'node:test';
import assert from 'node:assert/strict';
import { acceptTranslation } from '../src/lib/caption-feed.ts';
const item = (id, revision) => ({ segment: { id, revision }, result: { translated_text: `${id}-${revision}` } });
test('caption revisions replace in place and ignore stale translations', () => {
  const original = [item('a',1),item('b',1)];
  const updated = acceptTranslation(original,item('a',2));
  assert.deepEqual(updated.map(x=>x.result.translated_text),['a-2','b-1']);
  assert.equal(acceptTranslation(updated,item('a',1)),updated);
  assert.equal(acceptTranslation(updated,item('a',2)),updated);
});
