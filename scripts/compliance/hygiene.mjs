// Static checks for build hygiene and dependency supply.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { readText, root } from './lib.mjs';

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

const source = walk(join(root, 'src'))
  .map((path) => readFileSync(path, 'utf8'))
  .join('\n');

export const checks = [
  {
    id: 'static:sourcemap-config',
    run() {
      const config = readText('vite.config.ts');
      assert.match(
        config,
        /sourcemap:\s*!!process\.env\.TAURI_ENV_DEBUG/,
        'vite.config.ts must tie source maps to the Tauri debug flag'
      );
    },
  },
  {
    id: 'static:devtools-absent',
    run() {
      const cargo = readText('src-tauri', 'Cargo.toml');
      assert.ok(
        !/"devtools"/.test(cargo),
        'the Tauri devtools feature must stay off'
      );
    },
  },
  {
    id: 'static:dead-logo-path',
    run() {
      for (const token of [
        'logo.dev',
        'VITE_LOGO_API_TOKEN',
        'fetchBrandLogoAsBase64',
      ]) {
        assert.ok(!source.includes(token), `source must not contain ${token}`);
      }
    },
  },
  {
    id: 'static:deps-audit',
    run() {
      execSync('yarn npm audit --severity high', { cwd: root, stdio: 'ignore' });
    },
  },
  {
    id: 'static:deps-crypto',
    run() {
      const lock = readText('src-tauri', 'Cargo.lock');
      assert.match(lock, /name = "argon2"/);
      assert.match(lock, /name = "aes-gcm"/);
    },
  },
];
