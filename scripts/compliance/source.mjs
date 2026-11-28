// Static source checks for the renderer. Ids match
// docs/security/compliance-tests.md §3.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { root } from './lib.mjs';

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

const files = walk(join(root, 'src')).map((path) => ({
  path,
  rel: relative(root, path).replaceAll('\\', '/'),
  text: readFileSync(path, 'utf8')
}));

function hits(pattern) {
  return files.flatMap((file) =>
    [...file.text.matchAll(pattern)].map((match) => ({
      file: file.rel,
      match: match[0]
    }))
  );
}

function assertNone(pattern, message) {
  const found = hits(pattern);
  assert.equal(found.length, 0, `${message} (${JSON.stringify(found.slice(0, 3))})`);
}

export const checks = [
  {
    id: 'static:source-injection-sinks',
    run() {
      assertNone(
        /v-html|innerHTML|outerHTML|document\.write|insertAdjacentHTML|srcdoc|DOMParser|new\s+Function|\beval\s*\(|compile\s*\(/g,
        'no HTML/script sink may exist in src/'
      );
    },
  },
  {
    id: 'static:source-eval',
    run() {
      assertNone(/\beval\s*\(|new\s+Function/g, 'no eval or Function constructor may exist in src/');
    },
  },
  {
    id: 'static:source-postmessage',
    run() {
      assertNone(
        /postMessage|addEventListener\(\s*['"]message/g,
        'no renderer postMessage listener may exist in src/'
      );
    },
  },
  {
    id: 'static:source-storage',
    run() {
      assertNone(
        /localStorage|sessionStorage|indexedDB/g,
        'no web storage may be used in src/'
      );
    },
  },
  {
    id: 'static:source-navigation',
    run() {
      assertNone(
        /window\.open|target\s*=\s*['"]_blank['"]|:href|:action|formaction|<a\s/g,
        'no navigation sink may exist in src/'
      );
    },
  },
  {
    id: 'static:source-url-attrs',
    run() {
      for (const file of files) {
        let index = file.text.indexOf(':src');
        while (index !== -1) {
          const before = file.text.slice(Math.max(0, index - 300), index);
          assert.ok(
            before.includes('<img'),
            `every :src binding must be an <img> (${file.rel})`
          );
          index = file.text.indexOf(':src', index + 4);
        }
      }
    },
  },
  {
    id: 'static:source-url-secrets',
    run() {
      const routeFiles = files.filter(
        (file) =>
          file.rel.includes('router') ||
          file.rel.endsWith('useSearch.ts') ||
          file.rel.endsWith('useRouterHistory.ts')
      );

      for (const file of routeFiles) {
        assert.ok(
          !/password|secret|card_number/.test(file.text),
          `route state must not name a secret (${file.rel})`
        );
      }
    },
  },
  {
    id: 'static:source-zeroization',
    run() {
      assertNone(/\.fill\(0\)|zeroize\(/g, 'the renderer must not pretend to zero strings');
    },
  },
  {
    id: 'static:source-logs',
    run() {
      for (const file of files) {
        if (file.rel.endsWith('src/seed.ts')) continue; // dev fixture, not bundled

        for (const line of file.text.split(/\r?\n/)) {
          if (!line.includes('console.')) continue;

          assert.match(
            line,
            /console\.error\(\s*['"`]/,
            `console calls must be labelled errors (${file.rel})`
          );
          assert.ok(
            !/password|secret|card_number|masterPassword/i.test(line),
            `console calls must not name a secret (${file.rel})`
          );
        }
      }
    },
  },
  {
    id: 'static:source-password-inputs',
    run() {
      for (const rel of [
        'src/components/views/LoginView.vue',
        'src/components/views/RegisterView.vue'
      ]) {
        const file = files.find((entry) => entry.rel === rel);
        assert.ok(file, `${rel} must exist`);
        assert.match(
          file.text,
          /type="password"/,
          `${rel} must use a password input`
        );
      }
    },
  },
  {
    id: 'static:source-upload-accept',
    run() {
      const file = files.find((entry) => entry.rel === 'src/util/imageUpload.ts');
      assert.ok(file, 'imageUpload.ts must exist');
      assert.match(file.text, /input\.accept/);
      assert.match(file.text, /ACCEPTED_IMAGE_TYPES|file\.type/);
    },
  },
  {
    id: 'static:command-validation',
    run() {
      const commands = readFileSync(
        join(root, 'src-tauri', 'src', 'commands.rs'),
        'utf8'
      );
      assert.ok(
        !/serde_json::Value/.test(commands),
        'command arguments must be concrete types, not serde_json::Value'
      );
    },
  },
];
