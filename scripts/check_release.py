"""Check EigenTrail release metadata locally; no builds, account checks or uploads."""
from pathlib import Path,PurePosixPath
import argparse
from email.parser import BytesParser
from email.policy import default
from email.utils import getaddresses
import json
import re
import tarfile
import tomllib
import zipfile

ROOT=Path(__file__).resolve().parents[1]
CONTACT='gbontemps@protonmail.com'
REPOSITORY='https://github.com/TrompetteMarine/EigenTrail'
CLASSIFIERS={'Programming Language :: Python :: 3.14',
    'Programming Language :: Python :: Implementation :: CPython','Operating System :: MacOS :: MacOS X'}


def require(condition,message):
    if not condition:raise ValueError(message)


def readme(project):
    value=project.get('readme')
    require(isinstance(value,dict) and set(value)=={'text','content-type'},'README must be inline in pyproject.toml, without checkout-relative file paths')
    require(value['content-type']=='text/markdown' and isinstance(value['text'],str) and value['text'].strip(),'Missing Markdown long description')
    return value['text'].strip()


def dcf(path):
    fields={};key=None
    for line in path.read_text().splitlines():
        if line.startswith((' ','\t')) and key:fields[key]+=' '+line.strip()
        elif ':' in line:
            key,value=line.split(':',1);fields[key]=value.strip()
    return fields


def contacts(values):
    return [(entry.get('name'),entry.get('email')) for entry in values]


def core_metadata(data,project):
    require(len(data)<=2*1024**2,'Metadata exceeds2MiB')
    metadata=BytesParser(policy=default).parsebytes(data)
    require(not metadata.defects,'Malformed core metadata')
    for field,key in [('Name','name'),('Version','version'),('Summary','description')]:
        require(metadata.get(field)==project[key],'Changed artifact '+field)
    require(CONTACT in {email for _,email in getaddresses(metadata.get_all('Maintainer-email',[]))},'Missing real artifact maintainer email')
    require(metadata.get('License-Expression')=='MIT','Missing MIT license expression')
    require(metadata.get('Description-Content-Type','').split(';',1)[0].strip()=='text/markdown','Missing Markdown description type')
    body=metadata.get_payload(decode=True)
    require(isinstance(body,bytes) and body.decode('utf-8').strip()==readme(project),'Artifact long description differs from the Python project')
    requires=set(metadata.get('Requires-Python','').replace(' ','').split(','))
    require(requires=={'>=3.14','<3.15'},'Artifact advertises an unqualified Python version')
    dependencies=[re.sub(r'[ ()]','',value).lower() for value in metadata.get_all('Requires-Dist',[])]
    require(len(dependencies)==1 and dependencies[0].startswith('numpy') and set(dependencies[0][5:].split(','))=={'>=2.3','<3'},'Changed artifact NumPy dependency')
    require(CLASSIFIERS<=set(metadata.get_all('Classifier',[])),'Missing native CPython3.14 classifiers')
    urls=dict(value.split(',',1) for value in metadata.get_all('Project-URL',[]))
    require({key:value.strip() for key,value in urls.items()}==project['urls'],'Changed artifact project URLs')


def artifact(path,project):
    require(path.is_file(),'Missing artifact: '+str(path))
    if path.suffix=='.whl':
        with zipfile.ZipFile(path) as archive:
            names=archive.namelist()
            require(len(names)==len(set(names)),'Duplicate wheel member')
            selected=[name for name in names if name.endswith('.dist-info/METADATA')]
            require(len(selected)==1,'Expected one wheel metadata record')
            require(archive.getinfo(selected[0]).file_size<=2*1024**2,'Oversized wheel metadata')
            core_metadata(archive.read(selected[0]),project)
        kind='wheel'
    else:
        prefix=project['name']+'-'+project['version']
        with tarfile.open(path) as archive:
            members=archive.getmembers();names=[member.name for member in members]
            require(len(names)==len(set(names)),'Duplicate source archive member')
            for name in names:
                parts=PurePosixPath(name)
                require(not parts.is_absolute() and '..' not in parts.parts,'Unsafe source archive name')
            for name in [prefix+'/pyproject.toml',prefix+'/PKG-INFO']:
                member=archive.getmember(name)
                require(member.isfile() and member.size<=2*1024**2,'Invalid source metadata member')
            source=tomllib.loads(archive.extractfile(prefix+'/pyproject.toml').read().decode('utf-8'))['project']
            require(source==project,'Source archive changed project metadata')
            core_metadata(archive.extractfile(prefix+'/PKG-INFO').read(),project)
        kind='sdist'
    return {'path':str(path),'kind':kind,'metadata':'passed'}


def check_release(root,artifacts=()):
    root=Path(root).resolve()
    project=tomllib.loads((root/'python/pyproject.toml').read_text())['project']
    workspace=tomllib.loads((root/'Cargo.toml').read_text())['workspace']['package']
    r=dcf(root/'R/DESCRIPTION')
    require(project['name']=='eigentrail' and project['version']==workspace['version']==r['Version'],'Python/R/Rust versions disagree')
    require(('Gabriel Bontemps',CONTACT) in contacts(project.get('maintainers',[])),'Missing real Python maintainer')
    require(('Gabriel Bontemps',CONTACT) in contacts(project.get('authors',[])),'Missing real Python author')
    require(CONTACT in {email for _,email in getaddresses(workspace.get('authors',[]))},'Missing real Rust author contact')
    require(CONTACT in r.get('Authors@R','') or CONTACT in r.get('Maintainer',''),'Missing real R maintainer contact')
    require(workspace.get('repository')==REPOSITORY,'Missing canonical Rust repository URL')
    require(project['license']=='MIT' and project['license-files']==['LICENSE'] and (root/'python/LICENSE').is_file(),'Missing first-party Python license')
    require(project['requires-python']=='>=3.14,<3.15','Declare only qualified CPython3.14 support')
    require(project['dependencies']==['numpy>=2.3,<3'],'Changed supported NumPy requirement')
    classifiers=set(project.get('classifiers',[]))
    require(CLASSIFIERS<=classifiers and 'Operating System :: OS Independent' not in classifiers,'Unqualified platform classifiers')
    require(bool(project['description'].strip()),'Missing package description')
    readme(project)
    urls=project.get('urls',{})
    require({'Homepage','Source','Documentation','Issues'}<=urls.keys(),'Missing public project URLs')
    require(all(value==REPOSITORY or value.startswith(REPOSITORY+'/') for value in urls.values()),'Invalid public project URL')
    checked=[artifact(Path(path),project) for path in artifacts]
    return {'schema':'eigentrail.release-metadata-check.v1','status':'passed','version':project['version'],
        'maintainer':CONTACT,'artifacts':checked,'published':False,'release_qualified':False,
        'external_setup':'Registry ownership/credentials, mailbox verification and platform release gates are not checked by this command.',
        'scope':'Local source and Python artifact metadata only; no build, upload, legal review or platform qualification.'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root',type=Path,default=ROOT)
    parser.add_argument('--artifact',type=Path,action='append',default=[])
    args=parser.parse_args()
    try:result=check_release(args.root,args.artifact)
    except (ValueError,KeyError,TypeError,OSError,tarfile.TarError,zipfile.BadZipFile) as error:
        result={'schema':'eigentrail.release-metadata-check.v1','status':'failed','error':str(error),'published':False,'release_qualified':False}
    print(json.dumps(result,indent=2,allow_nan=False))
    raise SystemExit(0 if result['status']=='passed' else 1)
