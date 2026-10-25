"""Build a self-contained R source tree with all Rust path dependencies."""
import shutil,hashlib
import json
from pathlib import Path
from dependency_inventory import copy_notices, notice_inputs
ROOT=Path(__file__).resolve().parents[1]
inputs=[p for folder in ['crates','R'] for p in (ROOT/folder).rglob('*') if p.is_file()]
payload=set(inputs)|set(notice_inputs().values())|{ROOT/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','scripts/stage_r.py','scripts/dependency_inventory.py']}
fingerprint=hashlib.sha256(b''.join(str(p.relative_to(ROOT)).encode()+p.read_bytes() for p in sorted(payload))).hexdigest()[:12]
stage=ROOT/'build'/('eigentrail-'+fingerprint)
if stage.exists():raise SystemExit('Refusing to overwrite existing R stage')
shutil.copytree(ROOT/'R',stage)
rust=stage/'src/rust';rust.mkdir()
for name in ('Cargo.toml','Cargo.lock','rust-toolchain.toml'):shutil.copy2(ROOT/name,rust/name)
shutil.copytree(ROOT/'crates',rust/'crates')
# A reused Cargo target cache uses mtimes; older copied source times can look fresh.
for p in (rust/'crates').rglob('*'):
    if p.is_file():p.touch()
shutil.copy2(ROOT/'docs/dependency-inventory.json',stage/'dependency-inventory.json')
(stage/'configure').chmod(0o755)
manifest={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(inputs)}
manifest['Cargo.lock']=hashlib.sha256((ROOT/'Cargo.lock').read_bytes()).hexdigest()
(stage/'inst').mkdir(exist_ok=True)
copy_notices(stage/'inst')
# R installs its DCF license stub over inst/LICENSE; retain the full MIT text separately.
(stage/'inst/licenses/eigentrail').mkdir()
shutil.copy2(ROOT/'LICENSE',stage/'inst/licenses/eigentrail/LICENSE')
(stage/'inst/BUILD-SOURCES.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(stage)
