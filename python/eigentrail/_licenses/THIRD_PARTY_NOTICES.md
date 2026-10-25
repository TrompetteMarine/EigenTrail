# Third-party notices

First-party EigenTrail code is MIT licensed. Dependencies retain their own
licenses. Resolved versions and features are in [the dependency inventory](docs/dependency-inventory.json),
with upstream notice URLs, source commits and hashes in
[license sources](docs/license-sources.json). License texts are under licenses/.

Python and R distributions include the production dependency license texts,
first-party MIT text, this notice, and the Rust 1.99.0 standard-library
copyright/license collection. The compiler is a build prerequisite and is not
bundled. R retains its license stub and the full MIT text under
licenses/eigentrail/LICENSE inside the installed package.

Production disables inari's GMP feature. GMP 6.3.0 and MPFR 4.2.2 through
gmp-mpfr-sys 1.7.1 are development-only independent numerical test oracles.
Their licenses and native build requirements differ from those of production
artifacts. Running a local test or build does not determine redistribution
compliance for another platform/artifact.

Faer's embedded third-party notices, including COPYING.EIGEN.MPL2, are retained.
The pinned faer 0.24.4 source is
https://static.crates.io/crates/faer/faer-0.24.4.crate, SHA-256
5ab6df3dd147fe8d702a288b95bcd8fcc499ab572fc80da6828f60cd4d524d67.

When changing dependencies, update the inventory and license texts together.
`copy_notices` in scripts/dependency_inventory.py copies that retained production
set into Python/R packages; it does not discover or approve new dependencies.
