// Runs every static compliance check, prints each result, and exits non-zero
// if any check failed.
//
// A check prints its checklist id, so a red run maps straight back to a row
// in docs/security/audit-checklist.md.

import { checks as cspChecks } from './csp.mjs';
import { checks as capabilityChecks } from './capabilities.mjs';
import { checks as hygieneChecks } from './hygiene.mjs';
import { checks as sourceChecks } from './source.mjs';
import { checks as bundleChecks } from './bundle.mjs';

const checks = [
  ...cspChecks,
  ...capabilityChecks,
  ...hygieneChecks,
  ...sourceChecks,
  ...bundleChecks,
];

let failed = 0;
let skipped = 0;

for (const check of checks) {
  try {
    const result = await check.run();
    if (result === 'skip') {
      skipped += 1;
      console.log(`skip ${check.id}`);
    } else {
      console.log(`ok   ${check.id}`);
    }
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${check.id}: ${error.message}`);
  }
}

if (failed > 0) {
  console.error(`${failed} of ${checks.length} static checks failed`);
  process.exit(1);
}

console.log(
  `${checks.length - skipped} static checks passed${skipped ? `, ${skipped} skipped` : ''}`
);
