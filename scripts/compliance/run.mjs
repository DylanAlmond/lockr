// Runs every static compliance check and exits non-zero on the first failure.
//
// A check prints its checklist id, so a red run maps straight back to a row
// in docs/security/audit-checklist.md.

import { checks as cspChecks } from './csp.mjs';
import { checks as capabilityChecks } from './capabilities.mjs';

const checks = [...cspChecks, ...capabilityChecks];

let failed = 0;

for (const check of checks) {
  try {
    await check.run();
    console.log(`ok   ${check.id}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${check.id}: ${error.message}`);
  }
}

if (failed > 0) {
  console.error(`${failed} of ${checks.length} static checks failed`);
  process.exit(1);
}

console.log(`${checks.length} static checks passed`);
