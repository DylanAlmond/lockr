// Static checks for build hygiene and dependency supply.
//
// Each entry is `{ id, run }`; `run` throws with the observed value on
// failure. Ids match the rows in docs/security/compliance-tests.md.

import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { readJson, readText, root } from './lib.mjs';

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
  {
    id: 'static:deps-inventory',
    run() {
      execSync('node scripts/sbom.mjs --check', {
        cwd: root,
        stdio: 'ignore',
      });
    },
  },
  {
    id: 'static:release-signing-config',
    run() {
      const config = readJson('src-tauri', 'tauri.conf.json');
      const windows = config.bundle?.windows ?? {};

      assert.equal(
        windows.digestAlgorithm,
        'sha256',
        'Windows signing must use SHA-256'
      );
      assert.ok(
        windows.timestampUrl,
        'Windows signing must configure a timestamp server'
      );
      assert.equal(
        windows.certificateThumbprint,
        undefined,
        'the Windows certificate thumbprint is environment-specific and must come from CI'
      );

      const workflow = readText('.github', 'workflows', 'publish.yml');
      const active = workflow
        .split('\n')
        .filter((line) => !line.trimStart().startsWith('#'))
        .join('\n');

      for (const token of [
        'WINDOWS_CERTIFICATE',
        'Import-PfxCertificate',
        '$cert.Subject -eq $cert.Issuer',
        'verify-windows-signatures.ps1',
        'must be set for a release build (T-15)',
      ]) {
        assert.ok(
          active.includes(token),
          `the active release workflow must contain ${token}`
        );
      }

      // The macOS and Linux signing/verification blocks are intentionally
      // commented out for the Windows-only release. Their text may remain in
      // the file; their execution must not.
      for (const token of [
        'APPLE_CERTIFICATE',
        'APPLE_SIGNING_IDENTITY',
        'APPLE_TEAM_ID',
        'LINUX_GPG_PRIVATE_KEY',
        'APPIMAGETOOL_FORCE_SIGN',
        'verify-macos-signatures.sh',
        'verify-linux-signatures.sh',
      ]) {
        assert.ok(
          !active.includes(token),
          `${token} must stay disabled until macOS/Linux shipping resumes`
        );
      }

      assert.match(
        workflow,
        /Lockr currently ships on Windows only/,
        'the workflow must record the intentional Windows-only scope'
      );
      assert.match(
        workflow,
        /self-signed\s+#?\s*development certificate on purpose/,
        'the workflow must record that accepting the dev certificate is intentional'
      );
      assert.match(
        workflow,
        /TEMPORARY \(intentional\)/,
        'the self-signed trust block must be marked temporary'
      );
    },
  },
];
