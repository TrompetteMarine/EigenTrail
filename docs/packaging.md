# Build and publish EigenTrail

The maintainer is Gabriel Bontemps, <gbontemps@protonmail.com>. Native macOS ARM64
with CPython 3.14 and R 4.5 is tested. Linux requires Haswell/x86-64-v3 and suitable
OS instruction support. Native Mac and Linux artifact checks
[pass](https://github.com/TrompetteMarine/EigenTrail/actions/runs/40231849932);
execution on the minimum Linux CPU remains pending. Release only
artifacts that pass installation and execution on their declared platform.

## Build Python packages

Install Rust 1.99.0, native C/C++ tools, Python 3.14 and pinned maturin:

```sh
python -m pip install maturin==1.9.6
cd python
python -m maturin build --release --locked --out ../dist
cd ..
```

Build and stage the complete locked Python source distribution:

```sh
mkdir -p dist/raw
cd python
python -m maturin sdist --out ../dist/raw
cd ..
python3 scripts/stage_python_sdist.py dist/raw/eigentrail-0.1.0.tar.gz dist/eigentrail-0.1.0.tar.gz
```

Use a fresh output directory for each candidate. The staging helper refuses to
overwrite an archive and retains all four workspace members and the lockfile.
Install the staged archive into a fresh environment with
`python -m pip install dist/eigentrail-0.1.0.tar.gz`.

The PyPI long description is an inline Markdown table in
`python/pyproject.toml`. It travels inside the Python subtree's source metadata
and becomes the wheel `METADATA` and source `PKG-INFO` description. It has no
`../README.md` dependency. This uses the standard
[PEP 621 readme field](https://packaging.python.org/en/latest/specifications/pyproject-toml/#readme),
supported by [maturin 1.9.6](https://github.com/PyO3/maturin/blob/v1.9.6/src/metadata.rs).

Validate contact/version/URL metadata before building, then validate the actual
Python archives:

```sh
python3 scripts/check_release.py
python3 -m unittest discover -s scripts -p test_check_release.py -v
python3 scripts/check_release.py \
  --artifact dist/eigentrail-0.1.0.tar.gz \
  --artifact dist/eigentrail-0.1.0-cp314-cp314-macosx_14_0_arm64.whl
```

This stdlib check verifies local metadata and retained descriptions. It does not
build packages or establish registry access or platform qualification. Run the
manual [release-validation workflow](../.github/workflows/release-validation.yml)
from GitHub Actions for the candidate ref. It builds/tests native Mac and Linux
packages and uploads validation artifacts; it does not publish to a registry.
Its Linux CPU record must be assessed against the declared Haswell baseline.
Also retain clean wheel/source-install smoke results for each distributed platform.

The first intended PyPI artifacts are the qualified macOS ARM64 wheel and the
Python source distribution. Linux source installation must pass its clean build
and smoke checks before Linux source support is advertised. A Linux wheel built
with `--compatibility linux` is a CI validation artifact, not a manylinux-qualified
public wheel. Public Linux wheels require their own CPU, native-library and
distribution qualification before being added to the upload set.

## Linux source builds

The repository's `.cargo/config.toml` selects `target-cpu=haswell` for
`x86_64-unknown-linux-gnu`. The staged Python source distribution includes that
configuration, and the staged R source has the same target setting. The CPU and
OS must support the declared Haswell/x86-64-v3 baseline, including the interval
backend's AVX/FMA requirements.

Cargo ignores configuration inside dependency crates. After registry publication,
apply the target flag in the consuming application when adding/building the Rust
library or installing the checker:

```sh
export RUSTFLAGS="-C target-cpu=haswell"
cargo add eigentrail-core@=0.1.0
cargo build --release
cargo install eigentrail-check --version '=0.1.0' --locked
```

Keep that flag for subsequent builds of the consuming project. These commands
are for Linux x86-64; the repository's target-specific configuration leaves native
macOS ARM64 builds unchanged. Build success on a newer Linux CPU does not by
itself qualify execution on the minimum supported CPU.

## Build the R source package

The R package is declared for Unix platforms, including macOS and Linux.
With the Rust toolchain and required registry sources already cached, prepare an
isolated production workspace and its vendored dependencies:

```sh
python3 scripts/stage_r.py --prepare-vendor build/r-vendor-workspace
cargo metadata --offline --format-version 1 \
  --manifest-path build/r-vendor-workspace/Cargo.toml > build/r-vendor-metadata.json
cargo vendor --locked --offline --versioned-dirs \
  --manifest-path build/r-vendor-workspace/Cargo.toml \
  build/r-vendor-workspace/vendor > build/r-vendor-config.toml
eigentrail_r_stage="$(python3 scripts/stage_r.py --vendor-workspace build/r-vendor-workspace)"
R CMD build "$eigentrail_r_stage"
R CMD INSTALL eigentrail_0.1.0.tar.gz
R CMD check --as-cran eigentrail_0.1.0.tar.gz
```

Use a fresh vendor-workspace path. Preparation removes Python/development
dependencies from the copy; `cargo metadata` prunes that copy's lockfile before
the locked vendoring step. The repository lockfile stays unchanged. The staged
R package contains the production Rust sources, vendored registry sources and
offline Cargo configuration, so its installation needs no maintainer registry
cache. Check it in a clean library with the declared Rust/C/GNU make prerequisites.
Retain first-party MIT text, third-party notices, dependency licenses and Rust
standard-library notices in distributed source and native packages.

The native validation run has no R check errors or warnings. Mac reports one
incoming-submission/source-size note; Linux also reports the explicitly selected
`-march=haswell` flag as non-portable. Review the declared CPU/toolchain scope
with CRAN before submission; these results are not CRAN acceptance.

For offline Python development builds, prepare the Rust cache and Python
wheelhouse, set `CARGO_NET_OFFLINE=true`, `MATURIN_NO_INSTALL_RUST=1` and
`MATURIN_PEP517_ARGS="--locked --offline"`, then use pip's
`--no-index --find-links /path/to/wheelhouse` options. The R source package's
vendored dependency configuration is checked separately from this developer cache.

## Publish a checked candidate

Publication is a separate maintainer action. Keep the tested artifact bytes and
version fixed through the following steps; package metadata alone is not a
completed release.

1. **Rust:** publish `eigentrail-core` before `eigentrail-check`, since the checker
   depends on that exact core version. Inspect `cargo package --list`. Cargo 1.99
   can package both crates together before either is uploaded. For offline
   archive/build verification with cached dependencies, use
   `cargo package --locked --offline -p eigentrail-core -p eigentrail-check`.
   Run publication preflight with
   `cargo publish --dry-run --locked -p eigentrail-core -p eigentrail-check`;
   this can require registry metadata access even when dependency sources are
   cached. Neither command uploads. A publish dry run is passed only when its
   own command succeeds; registry authentication and actual publication remain
   separate steps.
   The Python/R binding crates have `publish=false`; their sources are bundled
   with their language packages. Cargo's
   [publication guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
   describes package verification and registry authentication.
2. **PyPI:** validate the exact wheel and staged source distribution with
   `python -m twine check` and install each in a clean environment. Configure a
   PyPI project owner and either Trusted Publishing or an upload credential.
   New projects can use a pending Trusted Publisher; see
   [PyPI publishing](https://docs.pypi.org/publishing/publishing-to-pypi/).
   Upload only the selected Python files, for example with
   `python -m twine upload dist/eigentrail-0.1.0.tar.gz dist/eigentrail-0.1.0-cp314-cp314-macosx_14_0_arm64.whl`.
   Do not include R or Cargo archives in that upload.
3. **CRAN:** submit the `R CMD build` source tarball after the required native
   checks and `R CMD check --as-cran` succeed. Address all errors, warnings and
   significant notes. CRAN uses its
   [submission form](https://cran.r-project.org/submit.html) and sends a
   confirmation to the designated maintainer; do not submit the local Mac binary.
   Check the current [CRAN policy](https://cran.r-project.org/web/packages/policies.html)
   before submission, including its portability requirements.

Registry name availability/ownership, verified mailbox access, credentials or
Trusted Publisher configuration, and CRAN acceptance are external setup steps.
They remain pending until actually completed. Store credentials outside the
repository. Local source builds remain available while registry setup and
unsupported-platform checks are pending.
