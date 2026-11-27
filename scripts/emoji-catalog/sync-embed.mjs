import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';

const catalog = new URL('../../assets/catalogs/emoji.json', import.meta.url);
const embedded = new URL('../../frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json', import.meta.url);
const hash = data => createHash('sha256').update(data).digest('hex');
const data = readFileSync(catalog);

const [option, extra] = process.argv.slice(2);
if (extra !== undefined || (option !== undefined && option !== '--check')) {
  throw new Error('Usage: node scripts/emoji-catalog/sync-embed.mjs [--check]');
}
if (option === '--check') {
  if (hash(readFileSync(embedded)) !== hash(data)) {
    throw new Error('Go character catalog differs from assets/catalogs/emoji.json. Run node scripts/emoji-catalog/sync-embed.mjs.');
  }
} else {
  writeFileSync(embedded, data);
}
