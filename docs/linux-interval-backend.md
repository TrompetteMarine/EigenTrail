# Linux interval backend candidate

The public workspace selects the preserved Inari 2.0.0 portability fork in
`backports/inari`. Linux workspace builds use `-C target-cpu=x86-64` and the
SSE2 directed-rounding backend. The Python import guard checks every reported
CPU for SSE2 before loading its native extension. The private Dev workspace
continues to use upstream Inari and keeps its GMP/MPFR validation environment.

The fork retains its MIT license. Its 33 source and notice files match the
original hash inventory byte for byte; [public reuse metadata](../licenses/inari-public-projection.json)
records those hashes and the upstream crate digest. Its original `R-only`
provenance describes the earlier R backport. This public projection reuses
those same bytes for workspace-built CLI and Python candidates. The separate
R archive keeps its own manifest, lock and build profile.

The SSE2 backend saves and restores the floating-point environment around
outward operations, including the Unix C99 `fma` call. This depends on the
declared toolchain and C library. Windows builds retain the separate
Haswell/OS-AVX profile and do not select this Unix fallback. AArch64 retains the
preserved implementation. Native endpoint, scientific, CPU-floor and external
library tests are required for each materially different shipped binary.

Generic Linux wheel publication remains held. A successful source build on a
new CPU does not establish execution on baseline hardware, a manylinux policy,
or numerical coverage. Qualification must include a baseline machine or an
explicitly identified emulated baseline, independent endpoint controls, the
installed legacy and rolling suites, I112, and `auditwheel` inspection of the
exact proposed wheel. Candidate receipts must distinguish these results.

For this workspace candidate, source builds use:

```sh
export RUSTFLAGS="-C target-cpu=x86-64"
export CFLAGS="-march=x86-64"
export CXXFLAGS="-march=x86-64"
```

Cargo workspace patches are not inherited by an external application's Git or
registry dependencies. Rust consumers outside this workspace must explicitly
select the matching fork or retain upstream Inari's qualified CPU profile.
Registry packaging smoke checks are separate from workspace-backend evidence.
