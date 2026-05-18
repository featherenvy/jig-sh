# Goal Harness

## Objective

Close the developer UX review loop by fixing all actionable findings from the current review.

## Verifiable Stopping Condition

Doctor no longer reports an irrelevant proxy error in stripped runtime mode, info distinguishes vault availability from initialization, tests and fixture validation pass, and final review finds no remaining actionable issues.

## Validation Loop

- scripts/jig check contract
- scripts/jig check test
- scripts/validate-fixtures.sh

## Constraints

- Keep existing repo ownership and generated harness behavior intact.
- Do not revert unrelated receipt state; append-only .agent state remains append-only.

## Checkpoints

- [ ] Phase 1: plan comprehensive fix.
- [ ] Phase 2: implement fixes.
- [ ] Phase 3: review all working changes.
- [ ] Phase 4: evaluate findings and either restart at phase 1 or declare goal met.

## Configured Jig Gates

- contract: check (jig.contract_check)
- tests: check (jig.test)

## Progress Log

- Goal harness created. Keep this section short and append dated checkpoints, failed attempts, and validation evidence.

## Notes

No extra notes.
