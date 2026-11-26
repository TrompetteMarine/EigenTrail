# Install EigenTrail for R

Install R package **eigentrail 0.3.0** from the `R` directory of GitHub tag `v0.3.0` to track eigenspaces between observed dates and independently check the resulting reports. The package builds from source. [Tagged R source](https://github.com/TrompetteMarine/EigenTrail/tree/v0.3.0/R)

## Requirements

Use R 4.5 or later on Apple silicon or Intel macOS, or x86-64 GNU/Linux. Install Cargo and rustc 1.85.1 or later, a C compiler, GNU make and tar with xz support. The package bundles its Rust sources and dependencies and compiles them offline. Its x86-64 backend uses the baseline SSE2 profile. Windows installations are unsupported (`OS_type: unix`).

## Install from GitHub

If you need the installer, run `install.packages("remotes")` once. In a fresh R session, install the tag and check the loaded version.

```r
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.3.0",
                        subdir = "R", upgrade = "never")
library(eigentrail)
stopifnot(as.character(packageVersion("eigentrail")) == "0.3.0")
```

The tag points to commit `778634ab999b4119230d59c602c0d8a0cc473c8f`. For the earlier 0.2.2 API, the release also provides an R source file named `eigentrail_0.2.2.tar.gz` and its checksum. [0.2.2 R release](https://github.com/TrompetteMarine/EigenTrail/releases/tag/v0.2.2)

## CRAN status

On 26 November 2026, CRAN listed version 0.2.2 in its incoming new-submission queue. Both the primary and cloud source indexes had no EigenTrail entry. Use the GitHub instructions above for 0.3.0. [CRAN incoming queue](https://cran.r-project.org/incoming/newbies/) · [CRAN package index](https://cran.r-project.org/web/packages/available_packages_by_name.html) · [Cloud source index](https://cloud.r-project.org/src/contrib/PACKAGES.gz)

## Check a rolling report

This example downloads a small synthetic request from the release tag. After downloading the request, the analysis and checker run locally.

```r
fixture <- tempfile(fileext = ".json")
download.file(paste0("https://raw.githubusercontent.com/TrompetteMarine/",
                     "EigenTrail/v0.3.0/examples/requests/rolling-lineage-request.json"),
              fixture, mode = "wb")
request <- paste(readLines(fixture, warn = FALSE), collapse = "\n")
report <- eigentrail::analyze_rolling_lineage_json(request)
stopifnot(isTRUE(eigentrail::check_rolling_lineage_json(request, report)))
```

The analysis returns a canonical JSON string. Both functions use the same request, including its dates, founder, angular tolerance and uncertainty assumptions. A tied group or an open comparison remains explicit in the report. [Rolling-lineage guide](rolling-lineage.md)

## Check a covariance history

```r
covariances <- array(rep(c(0.75, 0, 0, 0.25), 2), dim = c(2, 2, 2))
report <- eigentrail::track(covariances, times = c(0, 1),
                            radii = rep(2^-20, 2), lipschitz = 0)
stopifnot(is.character(report), length(report) == 1L)
```

The radii and regularity bound are supplied premises for this synthetic constant path. [Quickstart](quickstart.md) · [API](api.md) · [Calibration guide](curve-calibration.md)
