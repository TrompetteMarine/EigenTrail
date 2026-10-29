# Install EigenTrail

EigenTrail is currently installed from GitHub source. It is not yet distributed
through PyPI, CRAN or crates.io. Initial installation needs Git and internet access
to fetch source and build dependencies; analysis and checking run locally.

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
python -m pip install "eigentrail @ git+https://github.com/TrompetteMarine/EigenTrail.git#subdirectory=python"
python -c "import eigentrail; print('EigenTrail imported')"
```

The native Rust extension is compiled during installation. Keep the virtual
environment activated when running your analyses.

## R

Start R from the shell with the prerequisites above available:

```r
install.packages("remotes")
remotes::install_github("TrompetteMarine/EigenTrail", subdir = "R", upgrade = "never")
library(eigentrail)
```

The R subtree includes the Rust sources needed for its native extension. No
separate repository staging step is needed.

## Rust and command line

Install the command-line interface:

```sh
cargo install --locked --git https://github.com/TrompetteMarine/EigenTrail.git eigentrail-check
eigentrail-check --help
```

To use the Rust libraries in an application, add these Git dependencies to its
`Cargo.toml`:

```toml
[dependencies]
eigentrail-core = { git = "https://github.com/TrompetteMarine/EigenTrail.git" }
eigentrail-check = { git = "https://github.com/TrompetteMarine/EigenTrail.git" }
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
