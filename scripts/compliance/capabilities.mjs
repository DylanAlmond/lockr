// Static checks for the Tauri capability surface.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

const capability = JSON.parse(
  readFileSync(join(root, 'src-tauri', 'capabilities', 'default.json'), 'utf8')
);
const permissions = capability.permissions ?? [];
const cargo = readFileSync(join(root, 'src-tauri', 'Cargo.toml'), 'utf8');
const packageJson = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));

const EXPECTED_PERMISSIONS = [
  'core:app:allow-version',
  'core:event:default',
  'core:window:allow-close',
  'core:window:allow-is-maximized',
  'core:window:allow-minimize',
  'core:window:allow-start-dragging',
  'core:window:allow-toggle-maximize',
];

export const checks = [
  {
    id: 'static:capabilities',
    run() {
      const actual = [...permissions].sort();
      assert.deepEqual(
        actual,
        EXPECTED_PERMISSIONS,
        `capability permissions must equal the expected set (got ${JSON.stringify(actual)})`
      );
    },
  },
  {
    id: 'static:capabilities-opener',
    run() {
      const openerPermission = permissions.find((permission) =>
        permission.startsWith('opener:')
      );
      assert.equal(
        openerPermission,
        undefined,
        `opener capability must be removed (found ${openerPermission})`
      );
      assert.ok(
        !cargo.includes('tauri-plugin-opener'),
        'tauri-plugin-opener must not be a Cargo dependency'
      );
      assert.ok(
        !('@tauri-apps/plugin-opener' in (packageJson.dependencies ?? {})),
        '@tauri-apps/plugin-opener must not be a package dependency'
      );
    },
  },
];
