# EigenTrail

EigenTrail tracks covariance eigenvalues and eigenspaces with TRACE. Python, R
and Rust interfaces produce uncertainty bounds, association alternatives and
independently checked reports.

Version **0.3.0** adds observed-date [rolling lineage](docs/rolling-lineage.md),
founder reach and optional Python plots. It retains explicit missing slots, tied
blocks and unresolved outcomes. The [0.3.0 notes](docs/releases/v0.3.0.md) describe
the new capability. The R package's latest published release remains **v0.2.2**,
which does not contain the rolling additions.

## Install

For CPython 3.14:

```sh
python -m pip install "eigentrail[plot]==0.3.0"
```

Wheels need no Rust toolchain. They cover macOS 14 or later on Apple silicon,
Linux x86-64 with glibc 2.28 or later, and Windows x86-64 on Haswell-class
processors whose operating system enables AVX. EigenTrail's Linux native code uses
only baseline x86-64 (SSE2) instructions; current NumPy releases need x86-64-v2.
On Windows a packaged guard refuses unsupported processors before the native
extension loads. Python source builds need Rust 1.99.0 and native C/C++ build
tools; Windows source builds are not supported. R source requires R >=4.5 and
Cargo/rustc >=1.85.1, a C compiler, GNU make and tar with xz support. See
[installation](docs/installation.md) for exact CPU/backend profiles and
[R installation](docs/r-installation.md).

CRAN publication is not inferred from a submission confirmation. Its public
listing and the release receipt determine the R channel status.

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
[lineage summaries](docs/lineage-summary.md),
[observed-date rolling lineage](docs/rolling-lineage.md), or
[JSON format](docs/serialization.md).
Runnable examples are in [examples](examples/).

First-party code is [MIT licensed](LICENSE). Dependency licenses and
[third-party notices](THIRD_PARTY_NOTICES.md) accompany the package.
Report problems through [GitHub issues](https://github.com/TrompetteMarine/EigenTrail/issues).
Maintainer: [Gabriel Bontemps](mailto:gbontemps@protonmail.com).
