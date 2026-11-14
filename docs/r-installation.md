# Install EigenTrail for R

R package 0.2.2 supports macOS ARM64/Intel and GNU/Linux x86-64 with baseline
SSE2. It does not require AVX, FMA or Haswell build flags. Python and Rust 0.2.0
retain their [separate requirements](installation.md).

Source installation requires R 4.5 or later, Cargo and rustc 1.85.1 or later,
a C compiler, GNU make and tar with xz support. Install these tools before
installing the package. Bundled Rust dependencies build offline; the package
installer does not download a Rust toolchain.

## GitHub source

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.2.2",
                        subdir = "R", upgrade = "never")
library(eigentrail)
```

Alternatively, download `eigentrail_0.2.2.tar.gz` and its checksum from the
[R release](https://github.com/TrompetteMarine/EigenTrail/releases/tag/v0.2.2),
verify the checksum, then run:

```sh
R CMD INSTALL eigentrail_0.2.2.tar.gz
```

Use the named R source asset rather than GitHub's repository snapshot archives.
Start R from a shell without the Python/Rust Haswell `RUSTFLAGS` setting.

## CRAN status

CRAN publication is not yet confirmed. Use GitHub source until `eigentrail`
0.2.2 or a later version is listed on CRAN. Once publication is confirmed:

```r
install.packages("eigentrail", repos = "https://cloud.r-project.org")
```

Release checks cover R 4.6.2 and R-devel 4.7.0 on macOS ARM64 and Intel R under
Rosetta, plus R 4.6.2 on Linux. The release receipt records the final CPU
qualification scope and publication status.

## First call

```r
covariances <- array(rep(c(0.75, 0, 0, 0.25), 2), dim = c(2, 2, 2))
report <- track(covariances, times = c(0, 1),
                radii = rep(2^-20, 2), lipschitz = 0)
stopifnot(is.character(report), length(report) == 1L)
```

R returns canonical checked JSON. The error radii and regularity bound above
are declared premises for this synthetic constant path. Checks do not infer
sampling assumptions or force an eigenvector identity near ties. Unresolved
bounds and associations remain valid report data.

Continue with the [quickstart](quickstart.md), [API](api.md) and
[calibration and geometry guide](curve-calibration.md).
