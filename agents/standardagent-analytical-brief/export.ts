import { readFile, writeFile } from 'node:fs/promises';
import { exportTextProfile } from './index.ts';
const target = new URL('./text-profile.json', import.meta.url);
const serialized = `${JSON.stringify(await exportTextProfile(), null, 2)}\n`;
if (process.argv.includes('--check')) {
  if (await readFile(target, 'utf8') !== serialized) throw new Error('text_profile_out_of_date');
} else {
  await writeFile(target, serialized, { mode: 0o644 });
}
