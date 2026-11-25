# Install EigenTrail

Current source stages **0.3.0, an unreleased candidate**. It contains the new
observed-date rolling API and plotting extra. There is no v0.3.0 tag or registry
upload. The latest published [GitHub release is v0.2.2](https://github.com/TrompetteMarine/EigenTrail/releases/tag/v0.2.2)
for R maintenance; PyPI still distributes Python **0.2.0**. Published installation
commands below are labelled separately and retain the earlier API.

## Candidate artifacts

Install only a candidate whose receipt binds its exact SHA-256, source commit,
OS, architecture, CPython ABI, minimum CPU and numerical backend. A successful
build or import alone does not qualify the scientific computation. The final
receipt also lists unsupported tuples and remaining evidence gaps.

| Candidate tuple | Qualification and publication scope |
| --- | --- |
| CPython 3.14, macOS ARM64/macOS 14+ | Exact wheel/source installs and installed legacy/rolling/plot replay required |
| CPython 3.14, Windows x86-64 MSVC | Haswell/OS-AVX helper, DLL, backend and installed science checks required; held |
| CPython 3.14, GNU/Linux x86-64 | Public workspace stages baseline SSE2; native baseline, endpoint, manylinux and exact-artifact science checks required; generic publication held |
| R, macOS/Linux declared targets | Separate offline source build, current-R and installed scientific checks required |

R on Windows remains a separate distribution decision. The optional `plot`
extra adds Matplotlib >=3.10,<4. Imports are lazy; plots work offline without TeX.

After the named macOS artifact passes qualification, install verified local bytes
from a fresh CPython 3.14 environment:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install --only-binary=:all: "./candidate/eigentrail-0.3.0-cp314-cp314-macosx_14_0_arm64.whl[plot]"
```

A wheel needs no Rust toolchain. Qualify the source archive separately in another
fresh environment with Rust **1.99.0**, Cargo and native C/C++ tools:

```sh
python -m pip install "./candidate/eigentrail-0.3.0.tar.gz[plot]"
```

Python receives NumPy float64 arrays and requires NumPy >=2.3,<3. macOS source
builds require Xcode command-line tools. Linux source builds need the distribution's
C/C++ tools and Python development headers. For the **0.3.0 public workspace
candidate**, use:

```sh
export RUSTFLAGS="-C target-cpu=x86-64"
export CFLAGS="-march=x86-64"
export CXXFLAGS="-march=x86-64"
```

This selects the preserved portability fork and its Unix SSE2 rounding backend.
The Python guard checks every CPU's kernel-visible SSE2 flag before native import;
missing or unreadable CPU information refuses the import. These settings establish
a build profile, not baseline runtime evidence. See the
[backend scope and provenance](linux-interval-backend.md). Private Dev retains its
upstream/Haswell oracle profile. Workspace Cargo patches do not propagate into an
external application's Git/registry dependencies; Rust consumers must explicitly
choose and qualify their dependency/backend profile.

Windows candidates require Haswell instructions and OS AVX support on every
active logical CPU. The packaged baseline helper checks those conditions before
the extension loads. Missing helpers, inaccessible affinities or unsupported CPUs
raise `ImportError`. Windows source installation remains unqualified until its
helper-build route is included and tested; a candidate job is not a release.

## R candidate

R 0.3.0 source requires **R >=4.5**, **Cargo and rustc >=1.85.1**, a C compiler,
GNU make and tar with xz support. Its declared targets are macOS ARM64/Intel and
GNU/Linux x86-64 with baseline SSE2. Use the separate R profile and its receipt;
Python/CLI qualification does not substitute for R checks.

```sh
R CMD INSTALL ./candidate/eigentrail_0.3.0.tar.gz
```

R includes production Rust sources, dependencies, licenses and offline build
configuration. Installation does not download a Rust toolchain. Use the named
source package, not an automatically generated repository snapshot. See
[R installation](r-installation.md) for earlier published routes and CRAN status.

## Candidate Rust and CLI

Use the exact public checkout SHA recorded in the eventual candidate receipt.
From that verified checkout, build or install the CLI locally:

```sh
cargo build --release --locked -p eigentrail-check
cargo install --locked --path crates/eigentrail-check --root ./candidate/cli
```

The public workspace source and its lock select the candidate backend. Retain the
request and checked report with their hashes. Git installation must pin the full
qualified public commit SHA supplied by the receipt; no prospective v0.3.0 tag is
an installation target yet.

## Earlier published releases

These commands install the **earlier Python/Rust 0.2.0** API, without the rolling
additions. Its qualified wheel targets CPython 3.14 on macOS ARM64/macOS 14+:

```sh
python -m pip install --only-binary=eigentrail "eigentrail==0.2.0"
```

Separate source installation uses
`python -m pip install --no-binary=eigentrail "eigentrail==0.2.0"`, or its pinned
GitHub source:

```sh
python -m pip install "eigentrail @ git+https://github.com/TrompetteMarine/EigenTrail.git@v0.2.0#subdirectory=python"
cargo install --locked --git https://github.com/TrompetteMarine/EigenTrail.git --tag v0.2.0 eigentrail-check
```

Those old source builds require Rust 1.99.0 and native tools. Their Linux profile
retains Haswell instructions and OS AVX support with
`RUSTFLAGS="-C target-cpu=haswell"`; the 0.3.0 baseline candidate settings do not
retroactively change that release. Its Linux wheel publication remains held.

The **earlier R 0.2.2** GitHub source route is:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.2.2",
                        subdir = "R", upgrade = "never")
```

Its named source asset is `eigentrail_0.2.2.tar.gz`. CRAN availability must be
verified from the public package listing; submission confirmation does not imply
publication. See [R installation](r-installation.md).

## Examples and errors

Use the [quickstart](quickstart.md) and [rolling guide](rolling-lineage.md) with
the matching candidate version. The installed package runs outside the checkout.
A compiler/toolchain error, CPU refusal or loader failure concerns the recorded
build/runtime tuple. A malformed-input error concerns the request contract.
Open, unresolved and unavailable scientific outcomes are valid report data;
installation must not convert them into a positive decision.
