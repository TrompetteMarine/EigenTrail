# EigenTrail

EigenTrail tracks covariance eigenvalues and eigenspaces with TRACE. Python, R
and Rust interfaces produce uncertainty bounds, association alternatives and
independently checked reports.

Current source stages **0.3.0, an unreleased candidate**. It adds observed-date
[rolling lineage](docs/rolling-lineage.md), founder reach and optional Python
plots. It retains explicit missing slots, tied blocks and unresolved outcomes.
[Candidate notes](docs/releases/v0.3.0.md) describe the new capability and the
remaining qualification gates. There is no 0.3.0 tag or registry publication.

The latest published GitHub release remains **v0.2.2** for R maintenance; PyPI
still distributes Python **0.2.0**. Those earlier releases do not contain the
rolling additions. Historical release records stay unchanged.

## Install a qualified candidate

Use only local candidate bytes whose SHA-256 and exact platform tuple have passed
the candidate receipt. For CPython 3.14 on macOS ARM64, the proposed wheel name is:

```sh
python -m pip install --only-binary=:all: "./candidate/eigentrail-0.3.0-cp314-cp314-macosx_14_0_arm64.whl[plot]"
```

For the separately checked source packages:

```sh
python -m pip install "./candidate/eigentrail-0.3.0.tar.gz[plot]"
R CMD INSTALL ./candidate/eigentrail_0.3.0.tar.gz
```

Built wheels need no Rust toolchain. Python source requires Rust 1.99.0 and
native C/C++ build tools. R source requires R >=4.5 and Cargo/rustc >=1.85.1,
a C compiler, GNU make and tar with xz support. The public Linux workspace stages
a baseline SSE2 interval backend, with generic wheel publication held until its
native baseline, manylinux and scientific qualification. Windows MSVC/Haswell
candidates also remain held; source installation needs separate helper-build
qualification. See [installation](docs/installation.md) for exact CPU/backend
profiles, earlier published commands and [R installation](docs/r-installation.md).

To install the **earlier published Python 0.2.0** API from PyPI:

```sh
python -m pip install --only-binary=eigentrail "eigentrail==0.2.0"
```

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
