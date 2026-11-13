# Install EigenTrail

Install tagged source from GitHub using the commands below. Qualified package
assets and the actual status of each registry channel are listed on the
[release page](https://github.com/TrompetteMarine/EigenTrail/releases/tag/v0.1.0).
Initial source installation needs Git and internet access to fetch source and
build dependencies; analysis and checking run locally.

## Prerequisites

- Rust **1.99.0**, including `cargo`, and a native C/C++ toolchain.
- macOS **ARM64**, or GNU/Linux **x86-64** with Haswell/x86-64-v3 instructions and
  operating-system support for them.
- Python: **CPython 3.14**. Installation brings in NumPy **>=2.3,<3**.
- R: **R 4.5 or later**, GNU make, and tar with xz support.

On macOS, install the Xcode command-line tools if a compiler is missing. On Linux,
install your distribution's C/C++ build tools and development headers for the
Python or R interpreter you use.

Before any Linux source installation, set this in the shell used for the build:

```sh
export RUSTFLAGS="-C target-cpu=haswell"
```

Keep that setting for later Rust builds of applications using EigenTrail. Use the
Haswell target only on the declared Linux x86-64 platform; it does not apply to
macOS ARM64.

## Python

Create a virtual environment and install directly from the Python subtree:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install "eigentrail @ git+https://github.com/TrompetteMarine/EigenTrail.git@v0.1.0#subdirectory=python"
python -c "import eigentrail; print('EigenTrail imported')"
```

The native Rust extension is compiled during installation. Keep the virtual
environment activated when running your analyses.

## R

Start R from the shell with the prerequisites above available:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", ref = "v0.1.0",
                        subdir = "R", upgrade = "never")
library(eigentrail)
```

The R subtree includes the Rust sources needed for its native extension. No
separate repository staging step is needed.

For a qualified R source asset downloaded from the release page, install the
built package with `R CMD INSTALL eigentrail_0.1.0.tar.gz`. GitHub's automatically
generated repository snapshots are different files. R installation uses bundled
Rust dependencies offline, but requires a compatible installed Rust toolchain.

Linux wheels requiring Haswell instructions are distributed only as explicitly
qualified GitHub assets. A generic Linux wheel tag cannot select a compatible
CPU. A macOS ARM64 wheel requires macOS 14 or later and CPython 3.14. Registry
publication and a successful local build are separate qualification steps; check
the release's channel receipt before choosing a registry installation route.
Before loading native code, Python on Linux checks every CPU's kernel-visible
Haswell flags in `/proc/cpuinfo`. Missing or unreadable flags raise `ImportError`;
older kernels that omit flags can conservatively refuse a capable CPU. Source
builds still require the declared host and compiler prerequisites.

## Rust and command line

Install the command-line interface:

```sh
cargo install --locked --git https://github.com/TrompetteMarine/EigenTrail.git --tag v0.1.0 eigentrail-check
eigentrail-check --help
```

To use the Rust libraries in an application, add these Git dependencies to its
`Cargo.toml`:

```toml
[dependencies]
eigentrail-core = { git = "https://github.com/TrompetteMarine/EigenTrail.git", tag = "v0.1.0" }
eigentrail-check = { git = "https://github.com/TrompetteMarine/EigenTrail.git", tag = "v0.1.0" }
```

The core produces reports; the checker independently checks them. Retain your
application's `Cargo.lock` to keep its resolved dependencies fixed.

## Run the examples

The installed package can be used from any directory. To run the repository's
example scripts or use its request JSON files, also get a source checkout:

```sh
git clone https://github.com/TrompetteMarine/EigenTrail.git
cd EigenTrail
python examples/track_rotation.py
```

Continue with the [Python/R quickstart](quickstart.md). If a source build fails,
check the interpreter and Rust versions, native compiler availability, and the
Linux CPU target before reporting the error and platform in a
[GitHub issue](https://github.com/TrompetteMarine/EigenTrail/issues).

An error before compilation usually concerns Git access, dependency download or
the interpreter version. `cargo`/`rustc` missing or too old is a toolchain setup
failure; package installation does not install Rust. An AVX/FMA build error on
Linux requires the declared CPU and compiler flags, not a relaxed numerical
check. An import or dynamic-loader error after installation requires checking
the active environment, loaded package path and platform/ABI tag. Validation
errors from `track_history` concern array dtype/shape or the stated contract;
an ordinary `Unresolved` feasibility result is valid report data.
