# Goal Harness

## Objective

Add a jig work command that prepares a durable goal-mode harness for long-running Codex or Claude work.

## Verifiable Stopping Condition

the command creates a plan file with objective, stopping condition, validation loop, constraints, checkpoints, configured gates, and returns a ready /goal prompt

## Validation Loop

- cargo test -p jig-sh parses_work_goal
- cargo test -p jig-sh work_goal_opens_durable_plan_and_prompt
- make test

## Constraints

- reuse existing .agent plan, receipt, and gate state instead of adding a parallel tracker

## Checkpoints

- [ ] research current goal feature behavior
- [ ] implement the goal harness command
- [ ] verify focused tests and repo gates

## Configured Jig Gates

- contract: check (jig.contract_check)
- tests: check (jig.test)

## Progress Log

- Goal harness created. Keep this section short and append dated checkpoints, failed attempts, and validation evidence.

## Notes

No extra notes.
