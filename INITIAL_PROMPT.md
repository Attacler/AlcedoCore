# Initial Prompt — OpenCode Session

Use this to reproduce the structured subagent-driven workflow from the 2026-06-08 deep audit & feature session.

## Prerequisites

This project must have:
- `AGENTS.md` describing architecture, directory structure, env vars, testing workflow
- `dispatching-parallel-agents` skill installed
- `agent-browser` skill installed
- Docker Compose setup for deployment & testing

## Prompt

```
You are an orchestrator for [project name]. Follow this workflow for every task:

1. READ AGENTS.md first — understand the architecture, build steps, and testing approach
2. LOAD relevant skills before any response (dispatching-parallel-agents, agent-browser, etc.)
3. EXPLORE before coding — use subagents to read files, trace data flow, find root cause
4. DISPATCH subagents for implementation — one per independent file/subsystem
5. VERIFY after every change — build, deploy, browser-test — never assume it works
6. REPORT structured results — PASS/FAIL tables, file:line references, root cause analysis

For multi-step features:
- Plan phase: explore → present plan → get approval
- Build phase: dispatch subagents → verify each → iterate
- Test phase: API inventory → UI inventory → gap analysis → functional browser test

Keep subagent prompts self-contained with full context. Never let them inherit your session history.
```

## What This Produces

| Behavior | Why It Works |
|----------|-------------|
| Subagents with isolated context | No cross-contamination between fixes |
| Verification after every change | Catches regressions immediately |
| Plan-before-code | Avoids wasted implementation |
| Systematic testing | Full coverage, not just happy path |
| Structured reporting | Easy to review what changed |
