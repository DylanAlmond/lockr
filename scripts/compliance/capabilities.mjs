// Static checks for the Tauri capability surface.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { readJson, readText } from './lib.mjs';

const capability = readJson('src-tauri', 'capabilities', 'default.json');
const permissions = capability.permissions ?? [];
const cargo = readText('src-tauri', 'Cargo.toml');
const packageJson = readJson('package.json');

const commandsSource = readText('src-tauri', 'src', 'commands.rs');
const buildSource = readText('src-tauri', 'build.rs');

// The set from tauri-config-baseline.md §5, each entry with a call site.
const EXPECTED_CORE_PERMISSIONS = [
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
      const actual = permissions
        .filter((permission) => permission.startsWith('core:'))
        .sort();
      const expected = [...EXPECTED_CORE_PERMISSIONS].sort();
      assert.deepEqual(
        actual,
        expected,
        `core capability permissions must equal the expected set (got ${JSON.stringify(actual)})`
      );

      const nonCore = permissions.filter(
        (permission) => !permission.startsWith('core:')
      );
      assert.ok(
        nonCore.every((permission) => permission.startsWith('allow-')),
        `only app command permissions may be non-core (got ${JSON.stringify(nonCore)})`
      );
    },
  },
  {
    id: 'static:command-scope',
    run() {
      const declared = [
        ...commandsSource.matchAll(/#\[tauri::command\]\s*pub fn (\w+)/g),
      ].map((match) => match[1]);

      const inBuild = [...buildSource.matchAll(/"([a-z0-9_]+)"/g)].map(
        (match) => match[1]
      );

      const inCapability = permissions
        .filter((permission) => permission.startsWith('allow-'))
        .map((permission) => permission.slice('allow-'.length).replaceAll('-', '_'));

      const sorted = (values) => [...new Set(values)].sort();
      const expected = sorted(declared);

      assert.deepEqual(
        sorted(inBuild),
        expected,
        `build.rs AppManifest::commands must list every #[tauri::command] (got ${JSON.stringify(sorted(inBuild))})`
      );
      assert.deepEqual(
        sorted(inCapability),
        expected,
        `capability allow-* permissions must grant every command (got ${JSON.stringify(sorted(inCapability))})`
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
