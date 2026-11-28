// Static checks over the built renderer bundle. Run `vite build` first; the
// checks skip when dist/ is absent. Ids match docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { root } from './lib.mjs';

const dist = join(root, 'dist');

function distFiles() {
  if (!existsSync(dist)) return null;

  const walk = (dir) =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      return entry.isDirectory() ? walk(path) : [path];
    });

  return walk(dist);
}

function bundleText(paths) {
  return paths.map((path) => readFileSync(path, 'utf8')).join('\n');
}

export const checks = [
  {
    id: 'static:bundle-sourcemaps',
    run() {
      const paths = distFiles();
      if (!paths) return 'skip';

      assert.equal(
        paths.filter((path) => path.endsWith('.map')).length,
        0,
        'the bundle must contain no source maps'
      );
      assert.ok(
        !bundleText(paths).includes('sourceMappingURL'),
        'the bundle must contain no sourceMappingURL'
      );
    },
  },
  {
    id: 'static:bundle-secrets',
    run() {
      const paths = distFiles();
      if (!paths) return 'skip';

      const text = bundleText(paths);
      for (const token of [
        'logo.dev',
        'VITE_LOGO_API_TOKEN',
        'Dev seed data',
        'test12',
      ]) {
        assert.ok(!text.includes(token), `the bundle must not contain ${token}`);
      }
    },
  },
  {
    id: 'static:bundle-dead-code',
    run() {
      const paths = distFiles();
      if (!paths) return 'skip';

      const text = bundleText(paths);
      for (const token of ['seed.ts', 'MASTER_PASSWORD']) {
        assert.ok(!text.includes(token), `the bundle must not contain ${token}`);
      }
    },
  },
  {
    id: 'static:bundle-external-hosts',
    run() {
      const paths = distFiles();
      if (!paths) return 'skip';

      const allowed = new Set(['www.w3.org', 'vuejs.org', 'ipc.localhost']);
      const hosts = new Set();

      for (const match of bundleText(paths).matchAll(
        /https?:\/\/([^/"'\s\\)]+)/g
      )) {
        hosts.add(match[1]);
      }

      const unexpected = [...hosts].filter((host) => !allowed.has(host));
      assert.deepEqual(
        unexpected,
        [],
        `the bundle must not reference an external host (${unexpected.join(', ')})`
      );
    },
  },
];
