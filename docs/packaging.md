# Build source and native packages

Install Rust 1.99.0, native C/C++ tools, Python 3.14 and pinned maturin:

```sh
python -m pip install maturin==1.9.6
cd python
python -m maturin build --release --locked --out ../dist
cd ..
```

For a Python source distribution, start from the python directory and stage the
complete locked workspace:

```sh
mkdir -p dist/raw
cd python
python -m maturin sdist --out ../dist/raw
cd ..
python3 scripts/stage_python_sdist.py dist/raw/eigentrail-0.1.0.tar.gz dist/eigentrail-0.1.0.tar.gz
```

The staging helper preserves all four workspace members. Use new output paths;
it refuses to overwrite an archive. Install with
`python -m pip install dist/eigentrail-0.1.0.tar.gz`.

For R:

```sh
python3 scripts/stage_r.py
R CMD build <directory-printed-by-the-script>
R CMD INSTALL eigentrail_0.1.0.tar.gz
```

The staged R source includes all required Rust path dependencies and licenses.
Keep the first-party MIT text, third-party notices, dependency license collection
and Rust standard-library notices in distributed packages. License-source URLs
and hashes are retained under docs; these JSON files describe dependencies,
not analysis inputs.

For offline builds, prepare the Rust toolchain/registry cache and Python
wheelhouse first. Set `CARGO_NET_OFFLINE=true`, `MATURIN_NO_INSTALL_RUST=1` and
`MATURIN_PEP517_ARGS="--locked --offline"`, then use pip's
`--no-index --find-links /path/to/wheelhouse` options.

Native macOS ARM64 is tested. Linux Haswell/x86-64-v3 remains unqualified;
other CPU/OS/Python/R configurations require their own builds and execution.
