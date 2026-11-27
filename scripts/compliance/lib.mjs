// Shared path helpers for the static compliance modules.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

export function readText(...segments) {
  return readFileSync(join(root, ...segments), 'utf8');
}

export function readJson(...segments) {
  return JSON.parse(readText(...segments));
}
