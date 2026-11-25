# Install EigenTrail for R

Current source stages **0.3.0, an unreleased candidate** with checked observed-date
rolling lineage. It has no new Git tag, CRAN submission or published source asset.
The latest published GitHub release remains **v0.2.2**. Python's published 0.2.0
API and the 0.3.0 public workspace have [separate profiles](installation.md).

## Candidate source package

R source requires **R >=4.5**, **Cargo and rustc >=1.85.1**, a C compiler,
GNU make and tar with xz support. Declared targets are macOS ARM64/Intel and
GNU/Linux x86-64 with baseline SSE2. AVX, FMA and Haswell flags are not required.
Qualification must bind the exact R archive and installed native library to the
candidate receipt; another interface's successful build is not sufficient.

After the local candidate's digest and target have passed those checks:

```sh
R CMD INSTALL ./candidate/eigentrail_0.3.0.tar.gz
```

Bundled Rust sources and production dependencies build offline. Install the Rust
compiler beforehand; the package does not download or install a toolchain. Use
the named R source package rather than a repository snapshot. A future Git route
must pin the full public commit SHA from the candidate receipt.

The new raw JSON routes are `analyze_rolling_lineage_json(request)` and
`check_rolling_lineage_json(request, report)`. R retains canonical checked JSON.
The [rolling guide](rolling-lineage.md) describes premises, founder scope and
unresolved outcomes. Its angular agreement includes equality and stays distinct
from the earlier continuous-history motion cutoff.

## Earlier published R 0.2.2

The previous release retains its original API and baseline SSE2 profile:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.2.2",
                        subdir = "R", upgrade = "never")
library(eigentrail)
```

Alternatively, download `eigentrail_0.2.2.tar.gz` and its checksum from the
[v0.2.2 release](https://github.com/TrompetteMarine/EigenTrail/releases/tag/v0.2.2),
verify the exact bytes, then run `R CMD INSTALL eigentrail_0.2.2.tar.gz`.
Historical platform checks and their limits remain in that release's receipt.

## CRAN status

A submission confirmation is not a publication receipt. Check the public CRAN
listing for the actual version before using its installation route. When the
requested version is available:

```r
install.packages("eigentrail", repos = "https://cloud.r-project.org")
```

The 0.3.0 candidate must not be inferred to exist on CRAN. Any new submission is
part of the separately authorised release step.

## First call

```r
covariances <- array(rep(c(0.75, 0, 0, 0.25), 2), dim = c(2, 2, 2))
report <- track(covariances, times = c(0, 1),
                radii = rep(2^-20, 2), lipschitz = 0)
stopifnot(is.character(report), length(report) == 1L)
```

The radii and regularity bound are supplied premises for this synthetic constant
path. Checking does not infer sampling assumptions or invent an eigenvector
identity near a tie. Continue with the [quickstart](quickstart.md), [API](api.md)
and [calibration guide](curve-calibration.md).
