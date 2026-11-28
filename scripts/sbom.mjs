// Generate a CycloneDX dependency inventory from the committed lockfiles.
//
// `yarn sbom` writes docs/security/sbom.json; `node scripts/sbom.mjs --check`
// fails when the committed file no longer matches the lockfiles.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const output = join(root, 'docs', 'security', 'sbom.json');
const checkOnly = process.argv.includes('--check');

const components = new Map();

function add(name, version, ecosystem) {
  if (!name || !version) return;

  const key = `${ecosystem}:${name}@${version}`;
  if (components.has(key)) return;

  const encodedName =
    ecosystem === 'npm' && name.startsWith('@')
      ? `%40${name.slice(1).replace('/', '%2F')}`
      : name;

  components.set(key, {
    type: 'library',
    name,
    version,
    purl: `pkg:${ecosystem}/${encodedName}@${version}`,
    properties: [{ name: 'lockr:ecosystem', value: ecosystem }]
  });
}

// Yarn Berry resolutions, including patched packages.
for (const line of readFileSync(join(root, 'yarn.lock'), 'utf8').split(/\r?\n/)) {
  const npm = line.match(/^\s{2}resolution: "(.+)@npm:([^"]+)"$/);
  if (npm) {
    add(npm[1], npm[2], 'npm');
    continue;
  }

  const patched = line.match(/^\s{2}resolution: "(.+?)@patch:.+::version=([^&"]+)/);
  if (patched) add(patched[1], patched[2], 'npm');
}

// Cargo packages.
for (const block of readFileSync(join(root, 'src-tauri', 'Cargo.lock'), 'utf8').split('[[package]]').slice(1)) {
  const name = block.match(/^\s*name = "([^"]+)"/m)?.[1];
  const version = block.match(/^\s*version = "([^"]+)"/m)?.[1];
  add(name, version, 'cargo');
}

const sorted = [...components.values()].sort((a, b) => a.purl.localeCompare(b.purl));
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));

const sbom = {
  bomFormat: 'CycloneDX',
  specVersion: '1.5',
  version: 1,
  metadata: {
    component: { type: 'application', name: pkg.name, version: pkg.version }
  },
  components: sorted
};

const json = `${JSON.stringify(sbom, null, 2)}\n`;

if (checkOnly) {
  const current = readFileSync(output, 'utf8');
  if (current !== json) {
    console.error('SBOM is stale; run `yarn sbom`');
    process.exit(1);
  }
  console.log(`SBOM matches the lockfiles (${sorted.length} components)`);
} else {
  writeFileSync(output, json);
  console.log(`Wrote docs/security/sbom.json (${sorted.length} components)`);
}
