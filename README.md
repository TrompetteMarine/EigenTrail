# EigenTrail

EigenTrail tracks covariance eigenvalues and eigenspaces over time with the TRACE
method. It returns checked uncertainty bounds, persistent associations and
continuous-history summaries through Python, R and Rust.

## Install

Requires Rust **1.99.0** and a native C/C++ toolchain for source builds.
Python requires **CPython 3.14** and **NumPy >=2.3,<3**. From this repository:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install ./python
```

For R (R 4.5 or later, Rust and GNU make installed):

```sh
python3 scripts/stage_r.py
R CMD INSTALL <directory-printed-by-the-script>
```

For the standalone command-line checker:

```sh
cargo install --locked --path crates/eigentrail-check
eigentrail-check --help
```

Native macOS ARM64 is tested. Linux requires Haswell/x86-64-v3 and appropriate
OS instruction support; native Linux qualification is pending. Packages are not
yet published to PyPI, CRAN or crates.io.

Linux builds from this checkout and the staged Python/R source packages select
the Haswell CPU target. Cargo registry users must set
`RUSTFLAGS="-C target-cpu=haswell"` for their consuming builds and checker
installation; Cargo ignores configuration bundled inside a dependency. See the
[Linux source instructions](docs/packaging.md#linux-source-builds).
Linux binary wheels remain behind a separate qualification gate.

## Track a covariance path

```python
import numpy as np
import eigentrail

covariances = np.array([[[0.75, 0.0], [0.0, 0.25]]] * 2, dtype=np.float64)
report = eigentrail.track(
    covariances,
    times=[0.0, 1.0],
    radii=[2.0**-20] * 2,
    lipschitz=0.0,
)
print(report["eigensystems"][0]["values"])
print(report["segments"][0]["tubes"][0]["bound"])
```

This synthetic constant path has eigenvalues 0.75 and 0.25; its leading-projector
bound is below 0.00001. `radii` and `lipschitz` are supplied uncertainty and global
regularity assumptions. The package does not infer those assumptions from slopes.

Python arrays use `(dates, features, features)`; R arrays use
`(features, features, dates)`. Keep units and measurement coordinates fixed.
Missing observations are rejected. An unresolved association is a scientific
result, and does not force an arbitrary eigenvector identity.

See the [Python/R quickstart](docs/quickstart.md), [API guide](docs/api.md),
and [canonical report format](docs/serialization.md).

## Examples

```sh
python examples/track_rotation.py
python examples/continuous-history.py
python examples/track_history.py
```

Further examples cover [jumps](examples/jump-history.py),
[initialized transport](examples/initialized-transport.py), and
[conditional crossings](examples/transverse-crossing.py).

## Test and build

```sh
cargo test --release --locked
cargo fmt --all -- --check
cargo clippy --locked --lib --bins -- -D warnings
```

Tests retain independent numerical expectations and require a C compiler and GNU
make for their development-only GMP/MPFR oracle. Production builds do not enable
that oracle. Python/R source distributions are built with the small helpers in
[packaging instructions](docs/packaging.md).

First-party code is [MIT licensed](LICENSE). Required dependency licenses and
[third-party notices](THIRD_PARTY_NOTICES.md) accompany source and native packages.

Maintainer: [Gabriel Bontemps](mailto:gbontemps@protonmail.com).
See [build and publication instructions](docs/packaging.md) for local checks and
the remaining registry setup steps.
