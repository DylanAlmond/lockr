// Static checks for the Tauri capability surface.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.
//
// `static:command-scope` is deferred: build.rs has no `AppManifest::commands`
// yet (F-05), so the check would be red by design.

import assert from 'node:assert/strict';
import { readJson, readText } from './lib.mjs';

const capability = readJson('src-tauri', 'capabilities', 'default.json');
const permissions = capability.permissions ?? [];
const cargo = readText('src-tauri', 'Cargo.toml');
const packageJson = readJson('package.json');

// The set from tauri-config-baseline.md §5, each entry with a call site.
const EXPECTED_PERMISSIONS = [
  'core:app:allow-version',
  'core:event:default',
  'core:window:allow-close',
  'core:window:allow-internal-toggle-maximize',
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
      const expected = [...EXPECTED_PERMISSIONS].sort();
      assert.deepEqual(
        actual,
        expected,
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
