import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../..');
const source = readFileSync(join(root, 'apps/desktop/src/lib/geography.ts'), 'utf8');

assert.match(source, /'санкт-петербург': 'spb'/);
assert.match(source, /москва: 'moscow'/);
assert.match(source, /казань: 'kazan'/);
assert.doesNotMatch(source, /sankt-peterburg/);
console.log('ok city slug overrides present in geography.ts');
