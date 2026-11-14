# EigenTrail

EigenTrail tracks covariance eigenvalues and eigenspaces over time with the TRACE
method. Python, R and Rust interfaces return numerically checked uncertainty
bounds, persistent associations and continuous-history summaries.

Version 0.2.0 adds [window and temporal calibration, projector stability and
SPD geometry](docs/curve-calibration.md), with explicit premises and unresolved
outcomes. A small [composition example](examples/calibrated-window.py) is included.
R 0.2.2 corrects build cleanup and linking for CRAN incoming checks, retaining
the baseline x86-64 support and lower Rust compiler minimum added in R 0.2.1.
Current source manifests use version 0.2.2; the published Python package and
the pinned Python/Rust installation commands below remain at 0.2.0.

## Install from GitHub

Python and Rust 0.2.0 source installation requires **Rust 1.99.0** and native
C/C++ build tools. The qualified Python wheel targets macOS ARM64. Their Linux
source builds require Haswell/x86-64-v3 CPU and operating-system instruction
support; set `export RUSTFLAGS="-C target-cpu=haswell"` for those builds only.
Linux binary publication remains held.

For **CPython 3.14**:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install "eigentrail @ git+https://github.com/TrompetteMarine/EigenTrail.git@v0.2.0#subdirectory=python"
```

For **R 4.5 or later**, use Cargo and rustc **1.85.1 or later**, a C compiler,
GNU make and tar with xz support. R supports macOS ARM64/Intel and GNU/Linux
x86-64 with baseline SSE2; it needs no Haswell build flags:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.2.2",
                        subdir = "R", upgrade = "never")
```

CRAN publication is not yet confirmed; use GitHub source until it is available.
See [R installation](docs/r-installation.md) for source assets and the conditional
CRAN route. For Python, Rust and the command-line interface, see
[installation](docs/installation.md). Release receipts record qualified artifacts
and the actual status of each distribution channel.

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

This synthetic constant path has eigenvalues 0.75 and 0.25. Its leading-projector
error bound is below 0.00001 under the supplied error radii and global regularity
bound. Those are assumptions about the covariance path; observed slopes do not
establish them. [Gaussian calibration](docs/gaussian-calibration.md) is available
when its observation model applies.

Python arrays use `(dates, features, features)`; R arrays use
`(features, features, dates)`. Keep feature units and measurement coordinates
fixed. Missing values are rejected. Near ties, EigenTrail can return a separated
subspace or an unresolved association instead of forcing an eigenvector identity.

Continue with the [quickstart](docs/quickstart.md), [API guide](docs/api.md),
[lineage summaries](docs/lineage-summary.md), or [JSON format](docs/serialization.md).
Runnable examples are in [examples](examples/).

First-party code is [MIT licensed](LICENSE). Dependency licenses and
[third-party notices](THIRD_PARTY_NOTICES.md) accompany the package.
Report problems through [GitHub issues](https://github.com/TrompetteMarine/EigenTrail/issues).
Maintainer: [Gabriel Bontemps](mailto:gbontemps@protonmail.com).
