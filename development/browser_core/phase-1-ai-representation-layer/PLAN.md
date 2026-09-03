# Phase 1 — AI Representation Layer & API Shape

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Resolve the open design decision from plan §3 — which layer the human and AI representations are shared at — and, once decided, define the actual shape of the API the AI side receives. This decision gates Phase 5 (the AI representation output path can't be implemented until it's known what it's outputting), so it should be settled before Phase 3/4 implementation work gets far enough that the choice becomes expensive to change.

## Plan

Evaluate the four candidates from plan §3 against the project's actual goal (an AI agent perceiving *the same state* a human does, from the same render pass) rather than against generic browser-automation needs:

| Option | Fidelity to human perception | Structure for the AI | Engineering cost |
|---|---|---|---|
| Raw pixels + OCR/vision model | Highest | Lowest | Low (mostly glue) |
| DOM + CSSOM | Lower (misses visual-only state) | High | Low (mature tooling) |
| Accessibility Tree | Medium | High, standardized | Medium |
| Custom hybrid | Highest (by construction) | High | Highest |

A short spike on the accessibility-tree and/or custom-hybrid candidates (the two least "just wire up an existing library" options) is worth doing before committing, since their real engineering cost is the least certain of the four.

## Checklist

- [ ] Re-evaluate the four §3 candidates against MVP goals and against each other on fidelity / structure / cost
- [ ] Spike the accessibility-tree candidate against a toy DOM
- [ ] Spike the custom-hybrid candidate against a toy DOM
- [ ] Make the final decision and record the rationale by updating plan §3
- [ ] Define the AI-facing data format/schema for the chosen representation
- [ ] Define how the API exposes element identity/coordinates in a way an agent can act on (click, type, scroll)
- [ ] Define versioning/stability policy for the AI-facing API
- [ ] Define how this representation and human-facing UI state (e.g. highlights) stay synchronized, per the sync goal in plan §1
