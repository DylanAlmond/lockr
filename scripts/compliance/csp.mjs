// Static checks for the Tauri webview policy.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { readJson } from './lib.mjs';

const config = readJson('src-tauri', 'tauri.conf.json');
const security = config.app?.security ?? {};

const PRODUCTION_CONNECT = 'ipc: http://ipc.localhost';

function assertNoUnsafeSources(sources, directive) {
  assert.ok(
    !/unsafe-inline|unsafe-eval/.test(sources ?? ''),
    `${directive} must not allow 'unsafe-inline' or 'unsafe-eval' (got ${
      sources ?? 'nothing'
    })`
  );
}

export const checks = [
  {
    id: 'static:csp',
    run() {
      const csp = security.csp;
      assert.ok(csp && typeof csp === 'object', 'app.security.csp must be an object');

      assert.equal(csp['default-src'], "'self'");
      assert.equal(csp['script-src'], "'self'");
      assertNoUnsafeSources(csp['script-src'], "csp['script-src']");
      assert.equal(csp['object-src'], "'none'");
      assert.equal(csp['base-uri'], "'none'");
      assert.equal(csp['frame-ancestors'], "'none'");
      assert.equal(csp['form-action'], "'none'");
      assert.equal(csp['connect-src'], PRODUCTION_CONNECT);

      const img = csp['img-src'] ?? '';
      assert.ok(
        img.includes("'self'") && img.includes('data:'),
        `csp['img-src'] must be 'self' data: (got ${img || 'nothing'})`
      );
    },
  },
  {
    id: 'static:csp-dev',
    run() {
      const dev = security.devCsp;
      const csp = security.csp;
      assert.ok(dev && typeof dev === 'object', 'app.security.devCsp must be an object');

      for (const directive of [
        'default-src',
        'script-src',
        'object-src',
        'base-uri',
        'frame-ancestors',
        'form-action',
      ]) {
        assert.equal(
          dev[directive],
          csp[directive],
          `devCsp ${directive} must not weaken production`
        );
      }

      assertNoUnsafeSources(dev['script-src'], "devCsp['script-src']");
      assert.equal(dev['img-src'], csp['img-src']);
    },
  },
  {
    id: 'static:csp-injection',
    run() {
      assert.notEqual(
        security.dangerousDisableAssetCspModification,
        true,
        'Tauri asset CSP injection must stay enabled'
      );
    },
  },
  {
    id: 'static:headers',
    run() {
      const headers = security.headers ?? {};
      assert.equal(headers['X-Content-Type-Options'], 'nosniff');
      assert.equal(headers['Cross-Origin-Resource-Policy'], 'same-origin');
      assert.equal(headers['Cross-Origin-Opener-Policy'], 'same-origin');
      assert.ok(
        (headers['Permissions-Policy'] ?? '').includes('camera=()'),
        'Permissions-Policy must deny camera'
      );
    },
  },
  {
    id: 'static:freeze-prototype',
    run() {
      assert.equal(security.freezePrototype, true, 'freezePrototype must be true');
    },
  },
  {
    id: 'static:config-global-tauri',
    run() {
      assert.notEqual(
        config.app?.withGlobalTauri,
        true,
        'withGlobalTauri must stay off'
      );
    },
  },
];
