# EigenTrail

EigenTrail tracks covariance eigenvalues and eigenspaces over time with the TRACE
method. Python, R and Rust interfaces return numerically checked uncertainty
bounds, persistent associations and continuous-history summaries.

## Install from GitHub

Source installation requires **Rust 1.99.0** and native C/C++ build tools.
Supported platforms are macOS ARM64 and GNU/Linux x86-64 with
Haswell/x86-64-v3 CPU and operating-system instruction support.
On Linux, first set `export RUSTFLAGS="-C target-cpu=haswell"`.

For **CPython 3.14**:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install "eigentrail @ git+https://github.com/TrompetteMarine/EigenTrail.git#subdirectory=python"
```

For **R 4.5 or later**, with GNU make and tar with xz support installed:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", subdir = "R", upgrade = "never")
```

For Rust and the command-line interface, see [installation](docs/installation.md).
Current distribution is through GitHub source; EigenTrail is not yet available
from PyPI, CRAN or crates.io.

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
