# Goal Harness

## Objective

Close the fresh review loop by fixing the remaining doctor proxy readiness issue for configured dev-app repos in stripped runtime mode.

## Verifiable Stopping Condition

scripts/jig doctor can report proxy readiness for configured dev-app repos even when the current binary lacks the dev-proxy feature, the no-app path remains clean, validation passes, and final review finds no actionable findings.

## Validation Loop

- scripts/jig check contract
- scripts/jig check test
- scripts/validate-fixtures.sh

## Constraints

- Keep the normal stripped runtime profile for non-proxy commands unless a proxy check actually needs the full proxy-capable launcher.
- Keep append-only .agent state append-only and do not revert existing receipts.

## Checkpoints

- [ ] Phase 1: plan comprehensive fix.
- [ ] Phase 2: implement fixes.
- [ ] Phase 3: Codex review all working changes.
- [ ] Phase 4: evaluate findings and either restart at phase 1 or declare goal met.

## Configured Jig Gates

- contract: check (jig.contract_check)
- tests: check (jig.test)

## Progress Log

- Goal harness created. Keep this section short and append dated checkpoints, failed attempts, and validation evidence.

## Notes

No extra notes.
