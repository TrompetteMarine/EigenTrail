# Checked scalar continuity and rank implications

This small Lean project contains three implications about **given labelled
continuous real-valued curves**. It was independently compiled and audited on
native macOS ARM64 on 28 October 2026, with the statement assertions checked
separately from the returned proof bodies.

| Declaration | Checked statement |
|---|---|
| `order_reversal_has_collision` | Opposite strict endpoint orders imply equality at an interior time |
| `rank_above_constant` | A distinguished curve's strict-above count stays constant while it meets no other curve |
| `rank_change_has_collision` | A changed strict-above count implies a collision somewhere in the closed interval |

The namespace is `KGi.Lineage`. `rankAbove` counts strictly greater values in a
finite family; it is zero-based and preserves ties. The proved helper
`lt_iff_lt_of_no_collision` preserves pairwise strict order. Other curves may
collide with one another in the second theorem. Endpoint ties remain possible
in the third theorem, so its collision interval is closed.

## Reproduce

Use the official toolchain recorded in `project/lean-toolchain`. From the
repository root:

```sh
cd docs/formal/lineage/project
lake exe cache get
mkdir -p .lake/build/lib/lean/RequestProject
lake env lean --root=. -o .lake/build/lib/lean/RequestProject/Lineage.olean RequestProject/Lineage.lean
lake env lean RequestProject/AxiomAudit.lean
lake env lean StatementAudit.lean
```

These commands compile the leaf source directly, as in the successful check.
Keep `lake-manifest.json` unchanged and use these module paths. The axiom audit
checks the definition, all three targets and the helper. The permitted
foundational axioms are `propext`, `Classical.choice` and `Quot.sound`.
`StatementAudit.lean` checks all three requested types, the helper type and the
exact finite-filter counting definition.

The checked configuration is Lean **4.34.0**, compiler commit
`293d5d0c0c3f3dded4688b3ccd6a33939ac5102b`, and mathlib commit
`5ed2965256430c3649e86755f9576b54eca72435`, together with the supplied transitive
lockfile. On native macOS ARM64 the official Lean executable SHA-256 was
`1b370cfcbf44e80d1b004ab1b1ab9a4c73951f9f7c242140bcff9bc577576554`.
The exact `RequestProject/Lineage.lean` source SHA-256 is
`82c647195961129dc93302bcdd7a9cff4c4d323f6a33a0489d67fdaa41e0ed3d`.

The proof provider disclosed replacing the initially requested 4.28 pins with
these 4.34 pins. Independent checking used the actual returned configuration;
the original 4.28 configuration was not replayed. Dependency objects came from
the pinned upstream compiled cache, with unchanged fingerprints across the
proof checks. This records a scoped check rather than a clean rebuild of every
upstream dependency. Other compiler/platform executions need their own records.

## Scope

These implications constrain claims about continuous scalar branch labels.
Matrix eigenbranch existence and regularity, unique eigenline continuation
through a collision, observation-based recovery of hidden labels, statistical
coverage and correctness of the native Rust implementation require separate
arguments and checks. The package's numerical checker and history-feasibility
guards keep their own acceptance contracts.

Proof source was generated with Aristotle and independently reviewed and
checked. The compact project preserves the proof, dependency pins and statement
assertions; development conversations and provider job history are omitted.
