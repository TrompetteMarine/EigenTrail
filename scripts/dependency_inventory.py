"""Copy required license notices into Python/R distributions."""
import json,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def notice_inputs():
    inventory=json.loads((ROOT/'docs/dependency-inventory.json').read_text())
    third_party={(p['name'],p['version']) for p in inventory['packages'] if p['source']}
    selected={(name,version) for graph in inventory['production_graphs'].values()
        for name,versions in graph['packages'].items() for version in versions} & third_party
    files={'LICENSE':ROOT/'LICENSE','THIRD_PARTY_NOTICES.md':ROOT/'THIRD_PARTY_NOTICES.md',
        'dependency-inventory.json':ROOT/'docs/dependency-inventory.json',
        'license-sources.json':ROOT/'docs/license-sources.json',
        'licenses/rust-standard-library-1.99.0/COPYRIGHT-library.html':
            ROOT/'licenses/rust-standard-library-1.99.0/COPYRIGHT-library.html'}
    for name,version in sorted(selected):
        folder=ROOT/'licenses/dependencies'/(name+'-'+version)
        assert folder.is_dir() and any(folder.iterdir()),'Missing production notices: '+str(folder)
        for path in sorted(folder.iterdir()):
            if path.is_file():files[str(path.relative_to(ROOT))]=path
    return files


def copy_notices(destination):
    destination=Path(destination)
    for name,source in notice_inputs().items():
        target=destination/name;target.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(source,target)

