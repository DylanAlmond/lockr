#!/usr/bin/env node
/**
 * Regenerate THIRD-PARTY-NOTICES from the committed dependency graph.
 *
 * Rust: `cargo metadata --locked` resolves every package in src-tauri/Cargo.lock,
 * including target-specific and build dependencies that are not compiled on the
 * machine that happens to run this script.
 *
 * npm: every resolution in yarn.lock is included. Packages present in
 * node_modules are read from their published package.json. Platform-specific
 * optional packages that `yarn install` did not place on this host (esbuild and
 * rollup binaries for other operating systems, for example) are read from the
 * npm registry so the file stays complete for every supported platform.
 *
 * No licence-scanning dependency is used: only Node built-ins plus cargo and
 * yarn, both of which the project already requires.
 *
 * Usage:
 *   node scripts/generate-third-party-notices.mjs
 *
 * The command writes THIRD-PARTY-NOTICES at the repository root. Run it after
 * any change to src-tauri/Cargo.lock or yarn.lock, and review the diff.
 */

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  existsSync,
  readFileSync,
  readdirSync,
  writeFileSync
} from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const outputPath = join(repoRoot, 'THIRD-PARTY-NOTICES');

function fail(message) {
  console.error(`generate-third-party-notices: ${message}`);
  process.exit(1);
}

function sha256(relativePath) {
  const bytes = readFileSync(join(repoRoot, relativePath));
  return createHash('sha256').update(bytes).digest('hex');
}

function toolVersion(command, args) {
  try {
    return execFileSync(command, args, { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

function repositoryUrl(repository) {
  if (!repository) return '';
  if (typeof repository === 'string') return repository;
  return typeof repository.url === 'string' ? repository.url : '';
}

function normalizeSpdx(value) {
  return typeof value === 'string' ? value.trim().replace(/\s+/g, ' ') : '';
}

const ROOT_PACKAGE = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'));

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

const metadata = JSON.parse(
  execFileSync(
    'cargo',
    [
      'metadata',
      '--format-version',
      '1',
      '--manifest-path',
      join(repoRoot, 'src-tauri', 'Cargo.toml'),
      '--locked'
    ],
    { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 }
  )
);

const packagesById = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
const nodesById = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));

const reachableIds = new Set();
const pending = [metadata.resolve.root];
while (pending.length > 0) {
  const id = pending.pop();
  if (reachableIds.has(id)) continue;
  reachableIds.add(id);
  const node = nodesById.get(id);
  if (!node) fail(`cargo metadata has no resolve node for ${id}`);
  for (const dependency of node.dependencies) pending.push(dependency);
}

const rootNode = nodesById.get(metadata.resolve.root);
const directKinds = new Map();
for (const dependency of rootNode.deps) {
  const pkg = packagesById.get(dependency.pkg);
  if (!pkg) fail(`cargo metadata has no package for ${dependency.pkg}`);
  const kinds = directKinds.get(pkg.name) ?? new Set();
  for (const kind of dependency.dep_kinds) {
    kinds.add(kind.kind ?? 'normal');
  }
  directKinds.set(pkg.name, kinds);
}

function rustRelationship(name) {
  const kinds = directKinds.get(name);
  if (!kinds) return 'transitive';
  if (kinds.has('normal')) return 'direct';
  if (kinds.has('build')) return 'direct (build)';
  if (kinds.has('dev')) return 'direct (dev)';
  return 'direct';
}

const rustPackages = [];
for (const id of reachableIds) {
  if (id === metadata.resolve.root) continue;
  const pkg = packagesById.get(id);
  if (!pkg) fail(`cargo metadata has no package for ${id}`);

  const licence = normalizeSpdx(pkg.license);
  if (!licence) {
    fail(
      `crate ${pkg.name} ${pkg.version} declares neither license nor license_file; ` +
        'resolve the licence before regenerating this file'
    );
  }

  rustPackages.push({
    name: pkg.name,
    version: pkg.version,
    licence,
    relationship: rustRelationship(pkg.name),
    repository: repositoryUrl(pkg.repository),
    manifestPath: pkg.manifest_path
  });
}

rustPackages.sort((left, right) =>
  left.name === right.name
    ? left.version.localeCompare(right.version)
    : left.name.localeCompare(right.name)
);

// ---------------------------------------------------------------------------
// npm
// ---------------------------------------------------------------------------

/** Walk node_modules and index the package.json of every installed package. */
function installedNpmPackages() {
  const found = new Map();

  function visitNodeModules(directory) {
    let entries;
    try {
      entries = readdirSync(directory, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith('.')) continue;

      if (entry.name.startsWith('@')) {
        visitScope(join(directory, entry.name));
        continue;
      }

      visitPackage(join(directory, entry.name));
    }
  }

  function visitScope(scopeDirectory) {
    let entries;
    try {
      entries = readdirSync(scopeDirectory, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || entry.name.startsWith('.')) continue;
      visitPackage(join(scopeDirectory, entry.name));
    }
  }

  function visitPackage(packageDirectory) {
    const manifestPath = join(packageDirectory, 'package.json');
    if (existsSync(manifestPath)) {
      try {
        const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
        if (manifest.name && manifest.version) {
          found.set(`${manifest.name}@${manifest.version}`, manifest);
        }
        // A package can carry its own nested dependencies.
        const nested = join(packageDirectory, 'node_modules');
        if (existsSync(nested)) visitNodeModules(nested);
      } catch {
        // A malformed package.json is not a dependency we can attribute; the
        // lockfile pass below will fail loudly if the resolution is missing.
      }
    }
  }

  visitNodeModules(join(repoRoot, 'node_modules'));
  return found;
}

/**
 * yarn.lock resolutions look like `name@npm:1.2.3`, `@scope/name@npm:1.2.3`,
 * or a patched descriptor such as
 * `fsevents@patch:fsevents@npm%3A2.3.3#optional!builtin<...>`.
 */
function parseResolution(resolution) {
  if (resolution.includes('@workspace:')) return null;

  if (resolution.includes('@patch:')) {
    const name = resolution.slice(0, resolution.indexOf('@patch:'));
    const encoded = resolution.match(/@npm%3A([^#]+)/);
    if (!encoded) return null;
    return { name, version: decodeURIComponent(encoded[1]) };
  }

  if (!resolution.includes('@npm:')) return null;
  const separator = resolution.lastIndexOf('@');
  return {
    name: resolution.slice(0, separator),
    version: resolution.slice(separator + '@npm:'.length)
  };
}

const lockfile = readFileSync(join(repoRoot, 'yarn.lock'), 'utf8');
const resolutions = new Map();
for (const match of lockfile.matchAll(/^  resolution: "(.+)"$/gm)) {
  const parsed = parseResolution(match[1]);
  if (!parsed) continue;
  resolutions.set(`${parsed.name}@${parsed.version}`, parsed);
}

const installed = installedNpmPackages();

async function registryManifest(name, version) {
  const url = `https://registry.npmjs.org/${name}/${encodeURIComponent(version)}`;
  const response = await fetch(url);
  if (!response.ok) {
    fail(
      `npm registry returned ${response.status} for ${name}@${version}; ` +
        'run this command with network access so platform-specific packages ' +
        'that are absent from node_modules can still be attributed'
    );
  }
  return response.json();
}

function npmLicence(manifest) {
  const single = normalizeSpdx(manifest.license);
  if (single) return single;
  if (Array.isArray(manifest.licenses)) {
    const parts = manifest.licenses
      .map((entry) => normalizeSpdx(typeof entry === 'string' ? entry : entry?.type))
      .filter(Boolean);
    if (parts.length > 0) return parts.join(' OR ');
  }
  return '';
}

const directNpm = new Map();
for (const name of Object.keys(ROOT_PACKAGE.dependencies ?? {})) {
  directNpm.set(name, 'direct');
}
for (const name of Object.keys(ROOT_PACKAGE.devDependencies ?? {})) {
  if (!directNpm.has(name)) directNpm.set(name, 'direct (dev)');
}

const missingFromNodeModules = [...resolutions.entries()].filter(
  ([key]) => !installed.has(key)
);

// Fetch absent packages with a small concurrency limit so regeneration does not
// open dozens of sockets at once.
const CONCURRENCY = 8;
const fetched = new Map();
async function fetchMissing() {
  const queue = [...missingFromNodeModules];
  async function worker() {
    while (queue.length > 0) {
      const [key, { name, version }] = queue.shift();
      fetched.set(key, await registryManifest(name, version));
    }
  }
  await Promise.all(Array.from({ length: CONCURRENCY }, () => worker()));
}

await fetchMissing();

const npmPackages = [];
for (const [key, { name, version }] of resolutions) {
  const manifest = installed.get(key) ?? fetched.get(key);
  if (!manifest) {
    fail(
      `no metadata for ${key}; run "yarn install" and try again, or grant ` +
        'network access for packages this platform does not install'
    );
  }

  const licence = npmLicence(manifest);
  if (!licence) {
    fail(
      `npm package ${name}@${version} declares no licence; ` +
        'resolve the licence before regenerating this file'
    );
  }

  npmPackages.push({
    name,
    version,
    licence,
    relationship: directNpm.get(name) ?? 'transitive',
    repository: repositoryUrl(manifest.repository)
  });
}

npmPackages.sort((left, right) =>
  left.name === right.name
    ? left.version.localeCompare(right.version)
    : left.name.localeCompare(right.name)
);

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function escapeCell(value) {
  return String(value).replace(/\|/g, '\\|').replace(/\r?\n/g, ' ');
}

function table(packages) {
  const lines = [
    '| Package | Version | Licence | Relationship | Repository |',
    '| --- | --- | --- | --- | --- |'
  ];
  for (const pkg of packages) {
    lines.push(
      `| ${escapeCell(pkg.name)} | ${escapeCell(pkg.version)} | ${escapeCell(
        pkg.licence
      )} | ${escapeCell(pkg.relationship)} | ${escapeCell(pkg.repository)} |`
    );
  }
  return lines.join('\n');
}

function licenceCounts(entries) {
  const counts = new Map();
  for (const entry of entries) {
    counts.set(entry.licence, (counts.get(entry.licence) ?? 0) + 1);
  }
  return [...counts.entries()].sort(
    (left, right) => right[1] - left[1] || left[0].localeCompare(right[0])
  );
}

function licenceTable(entries) {
  const lines = ['| Licence | Packages |', '| --- | ---: |'];
  for (const [licence, count] of licenceCounts(entries)) {
    lines.push(`| ${escapeCell(licence)} | ${count} |`);
  }
  return lines.join('\n');
}

function namedCrates(entries, predicate) {
  return entries
    .filter((entry) => predicate(entry.licence))
    .map((entry) => `\`${entry.name}\` ${entry.version} (${entry.licence})`);
}

const allEntries = [...rustPackages, ...npmPackages];
const mplCrates = namedCrates(rustPackages, (licence) => licence.includes('MPL-2.0'));
const lgplAlternative = namedCrates(rustPackages, (licence) => licence.includes('LGPL'));
const bsd3Crates = namedCrates(rustPackages, (licence) => licence.includes('BSD-3-Clause'));
const bsd3Npm = npmPackages
  .filter((entry) => entry.licence.includes('BSD-3-Clause'))
  .map((entry) => `\`${entry.name}\` ${entry.version} (${entry.licence})`);
const oflNpm = npmPackages
  .filter((entry) => entry.licence.includes('OFL-1.1'))
  .map((entry) => `\`${entry.name}\` ${entry.version} (${entry.licence})`);

const dalekCrates = ['ed25519-dalek', 'curve25519-dalek'];
for (const name of dalekCrates) {
  if (!rustPackages.some((pkg) => pkg.name === name)) {
    fail(
      `${name} is no longer in the dependency graph. Update the BSD-3-Clause ` +
        'appendix and the obligation note in this generator before regenerating.'
    );
  }
}

function licenceText(packageName) {
  const pkg = rustPackages.find((entry) => entry.name === packageName);
  if (!pkg) fail(`cannot locate ${packageName} in cargo metadata`);
  const path = join(dirname(pkg.manifestPath), 'LICENSE');
  if (!existsSync(path)) {
    fail(
      `the source of ${packageName} is not unpacked (${path} is missing); ` +
        'run "cargo fetch" or a build, then regenerate'
    );
  }
  return readFileSync(path, 'utf8')
    .trim()
    .split(/\r?\n/)
    .map((line) => line.replace(/[ \t]+$/, ''))
    .join('\n');
}

const generatedOn = new Date().toISOString().slice(0, 10);
const cargoVersion = toolVersion('cargo', ['--version']);
const nodeVersion = process.version;
const yarnVersion =
  typeof ROOT_PACKAGE.packageManager === 'string'
    ? ROOT_PACKAGE.packageManager.replace(/^yarn@/, '')
    : 'unknown';

const header = `# Third-Party Notices

Lockr itself is proprietary software. See \`LICENSE\`: Copyright (c) 2026
Dylan Almond, all rights reserved. This file covers only third-party components.

This file is **generated** from the committed dependency graph. Do not edit it by
hand. Regenerate it whenever \`src-tauri/Cargo.lock\` or \`yarn.lock\` changes —
adding, removing, upgrading or downgrading a dependency, changing a feature flag
that pulls in a new optional dependency, or updating a build tool. A release
built from a lockfile that does not match the hashes below must not ship this
file as its notices.

## How this file was generated

\`\`\`sh
# from the repository root
yarn install
cargo fetch --manifest-path src-tauri/Cargo.toml --locked
node scripts/generate-third-party-notices.mjs
\`\`\`

The generator uses only Node built-ins and the project's existing toolchain. It
does not add a licence-scanning dependency. \`cargo fetch\` unpacks the crate
sources so the BSD-3-Clause appendix can be read from them.

- Rust inventory: \`cargo metadata --format-version 1 --locked\` over
  \`src-tauri/Cargo.lock\`, walking every resolved dependency (including build
  and target-specific dependencies). No platform filter is applied, so the
  inventory covers every platform Lockr is built for.
- npm inventory: every \`resolution\` in \`yarn.lock\`, paired with the
  \`package.json\` installed in \`node_modules\`. Platform-specific optional
  packages that this host did not install are fetched from the npm registry by
  exact name and version; generation therefore needs network access when those
  packages are absent.
- The command fails rather than writing a partial file if a package declares no
  licence, if an expected package disappears, or if a licence text required by
  the appendix cannot be read.

Generation inputs:

| Input | SHA-256 |
| --- | --- |
| \`src-tauri/Cargo.lock\` | \`${sha256('src-tauri/Cargo.lock')}\` |
| \`yarn.lock\` | \`${sha256('yarn.lock')}\` |

Generated on ${generatedOn} with ${cargoVersion}, Node ${nodeVersion} and Yarn
${yarnVersion}.

## Inventory summary

- Rust crates: ${rustPackages.length}
- npm packages: ${npmPackages.length}
- Total entries: ${allEntries.length}

### Rust crates by declared licence

${licenceTable(rustPackages)}

### npm packages by declared licence

${licenceTable(npmPackages)}

A slash in a Cargo licence expression (for example \`MIT/Apache-2.0\`) is the
historical form of \`OR\`. Licences are reproduced exactly as each package
declares them; no expression has been simplified or reinterpreted.

## Copyleft and notice obligations

Most of the graph is permissively licensed (MIT, Apache-2.0, ISC, BSD, Zlib,
CC0, Unicode-3.0). The following need attention when binaries are distributed.

### BSD-3-Clause: retain the copyright notice and disclaimer

\`ed25519-dalek\` and its \`curve25519-dalek\` dependency are retained for Device
Registry signing. Both are **BSD-3-Clause**, not MIT or Apache-2.0. Distributing
a Lockr binary that includes them requires reproducing the copyright notice,
the three conditions, and the disclaimer in the documentation or other materials
shipped with the binary. The full texts are in the appendix below, and must stay
there.

The other Rust crates that declare a BSD-3-Clause obligation are:

${bsd3Crates.map((entry) => `- ${entry}`).join('\n')}

npm packages that declare BSD-3-Clause:

${bsd3Npm.length > 0 ? bsd3Npm.map((entry) => `- ${entry}`).join('\n') : '- None'}

BSD-3-Clause is incompatible with a "no notice" distribution: dropping this
file, or trimming the appendix, does not satisfy the obligation.

### MPL-2.0: file-level copyleft, no effect on Lockr's licence

These crates are **MPL-2.0**:

${mplCrates.map((entry) => `- ${entry}`).join('\n')}

They arrive through Tauri's build/codegen tooling and \`wry\`'s DOM query stack,
not through Lockr's sync or cryptography code. MPL-2.0 is file-level copyleft:
it does not require Lockr's source to be published, but modified MPL-covered
files must be made available under MPL-2.0. Lockr does not modify them.

### LGPL-2.1-or-later: an alternative, not the chosen licence

These crates offer LGPL-2.1-or-later as one branch of a disjunction:

${lgplAlternative.map((entry) => `- ${entry}`).join('\n')}

Lockr takes the MIT or Apache-2.0 option, so the LGPL branch does not apply.
Do not relicense them under LGPL.

### OFL-1.1: bundled font

${oflNpm.length > 0 ? oflNpm.map((entry) => `- ${entry}`).join('\n') : '- None'}

The SIL Open Font License permits bundling and redistribution, but the font
must travel with its licence and must not be sold on its own. The package ships
its own licence file; retain it in any distribution that includes the font.

### Everything else

The remaining licences (MIT, Apache-2.0, ISC, BSD-2-Clause, Zlib, 0BSD,
CC0-1.0, Unicode-3.0, BlueOak-1.0.0) are permissive. Their attribution and
notice requirements are met by keeping this inventory with the distribution and
by not removing the licence files that ship inside the packages.

## Direct Rust dependencies

${table(rustPackages.filter((pkg) => pkg.relationship.startsWith('direct')))}

## Transitive Rust dependencies

${table(rustPackages.filter((pkg) => pkg.relationship === 'transitive'))}

## Direct npm dependencies

${table(npmPackages.filter((pkg) => pkg.relationship.startsWith('direct')))}

## Transitive npm dependencies

${table(npmPackages.filter((pkg) => pkg.relationship === 'transitive'))}

## Appendix: BSD-3-Clause texts for the dalek crates

### ed25519-dalek 2.2.0

\`\`\`text
${licenceText('ed25519-dalek')}
\`\`\`

### curve25519-dalek 4.1.3

\`\`\`text
${licenceText('curve25519-dalek')}
\`\`\`
`;

writeFileSync(outputPath, header, 'utf8');
console.log(
  `Wrote ${relative(repoRoot, outputPath)}: ${rustPackages.length} Rust crates, ` +
    `${npmPackages.length} npm packages`
);
