#!/usr/bin/env python3
"""M1 private install and user-job prototypes; labeled fake service only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import sys

FORMAT = 'pio-private-install/1'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def absolute(path):
    return Path(os.path.abspath(path))

def exists(path):
    return os.path.lexists(path)

def plain_parents(path):
    for parent in [path, *path.parents]:
        if parent.is_symlink():
            raise ValueError(f'unsafe symlink component: {parent}')

def sync_dir(path):
    fd=os.open(path,os.O_RDONLY)
    try:os.fsync(fd)
    finally:os.close(fd)

def exclusive(path, data, mode=0o600):
    fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,mode)
    with os.fdopen(fd,'wb') as file:
        file.write(data);file.flush();os.fsync(file.fileno())
    sync_dir(path.parent)

def install(binary, prefix, bin_dir, name):
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9._-]*',name):raise ValueError('invalid executable name')
    prefix,bin_dir=absolute(prefix),absolute(bin_dir)
    plain_parents(prefix);plain_parents(bin_dir)
    if not bin_dir.is_dir() or not prefix.parent.is_dir():raise ValueError('create installation parent and bin directory first')
    alias=bin_dir/name
    collisions=[str(alias)] if exists(alias) else []
    for folder in os.environ.get('PATH','').split(os.pathsep):
        candidate=absolute(Path(folder or '.')/name)
        if exists(candidate) and os.access(candidate,os.X_OK):collisions.append(str(candidate))
    if collisions:raise ValueError('placement_collision: '+', '.join(sorted(set(collisions)))+'; choose another --bin-name or private bin directory')
    if exists(prefix):raise ValueError(f'placement_collision: installation prefix exists: {prefix}')
    # This skeleton refuses upgrades. Every destination creation is exclusive.
    payload=prefix/'bin/pio'
    data=binary.read_bytes()
    created=[]
    try:
        prefix.mkdir(mode=0o700);created.append(prefix)
        payload.parent.mkdir(mode=0o700);created.append(payload.parent)
        exclusive(payload,data,0o700);created.append(payload)
        os.symlink(str(payload),alias);created.append(alias);sync_dir(bin_dir)
        manifest=dict(format=FORMAT,source='labeled fake service; no real adapter',binary_sha256=sha(payload),binary='bin/pio',bin_dir=str(bin_dir),bin_name=name,alias=str(alias),alias_target=str(payload))
        exclusive(prefix/'install.json',(json.dumps(manifest,indent=2)+'\n').encode());created.append(prefix/'install.json')
        exclusive(prefix/'install.sha256',(sha(prefix/'install.json')+'\n').encode());created.append(prefix/'install.sha256')
        sync_dir(prefix.parent)
        return manifest
    except Exception:
        for path in reversed(created):
            if path.is_dir() and not path.is_symlink():path.rmdir()
            else:path.unlink()
        raise

def read_manifest(prefix):
    prefix=absolute(prefix);plain_parents(prefix)
    manifest_path=prefix/'install.json'
    if not stat.S_ISREG(manifest_path.lstat().st_mode):raise ValueError('unowned manifest')
    receipt=prefix/'install.sha256'
    if not stat.S_ISREG(receipt.lstat().st_mode) or receipt.read_text().strip()!=sha(manifest_path):raise ValueError('modified_owned_file: manifest; no files removed')
    manifest=json.loads(manifest_path.read_text())
    if set(manifest)!={'format','source','binary_sha256','binary','bin_dir','bin_name','alias','alias_target'}:raise ValueError('unowned manifest fields')
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9._-]*',manifest['bin_name']) or manifest['alias']!=str(absolute(manifest['bin_dir'])/manifest['bin_name']):raise ValueError('unowned alias layout')
    if manifest['format']!=FORMAT or manifest['binary']!='bin/pio' or manifest['alias_target']!=str(prefix/'bin/pio'):raise ValueError('unowned layout')
    return manifest

def uninstall(prefix):
    prefix=absolute(prefix);manifest=read_manifest(prefix)
    payload=prefix/'bin/pio';alias=Path(manifest['alias'])
    plain_parents(payload.parent);plain_parents(alias.parent)
    # Validate the complete removal set before deleting any owned file.
    if not stat.S_ISREG(payload.lstat().st_mode) or sha(payload)!=manifest['binary_sha256']:raise ValueError('modified_owned_file: binary; no files removed')
    if not alias.is_symlink() or os.readlink(alias)!=str(payload):raise ValueError('modified_owned_file: alias; no files removed')
    alias.unlink();sync_dir(alias.parent)
    payload.unlink();(prefix/'install.json').unlink();(prefix/'install.sha256').unlink()
    for directory in (payload.parent,prefix):
        try:directory.rmdir()
        except OSError:pass  # State or unowned additions are never recursively removed.
    sync_dir(prefix.parent)
    return dict(removed=['bin/pio','install.json','install.sha256',str(alias)],preserved='all state, config, and unowned additions')

def render(prefix, config, data, socket, label, kind):
    if not re.fullmatch(r'[A-Za-z0-9._-]+',label):raise ValueError('invalid job label')
    read_manifest(prefix)
    args=[str(absolute(prefix)/'bin/pio'),'serve-fake','--data-dir',str(absolute(data)),'--config',str(absolute(config)),'--socket',str(absolute(socket))]
    if kind=='launchd':
        return plistlib.dumps(dict(Label=label,ProgramArguments=args,RunAtLoad=True,KeepAlive=True,StandardOutPath=str(absolute(data)/'service.stdout'),StandardErrorPath=str(absolute(data)/'service.stderr')))
    def quote(value):
        if '\n' in value or '\r' in value:raise ValueError('newline in service argument')
        return '"'+value.replace('\\','\\\\').replace('"','\\"').replace('%','%%').replace('$','$$')+'"'
    return ('[Unit]\nDescription=PIO labeled fake process service (M1 prototype)\n[Service]\nType=simple\nExecStart='+ ' '.join(map(quote,args))+'\nRestart=on-failure\nRestartSec=1\nTimeoutStopSec=10\nKillMode=process\n[Install]\nWantedBy=default.target\n').encode()

def main():
    parser=argparse.ArgumentParser(description=__doc__);commands=parser.add_subparsers(dest='command',required=True)
    add=commands.add_parser('install');add.add_argument('--binary',type=Path,required=True);add.add_argument('--prefix',type=Path,required=True);add.add_argument('--bin-dir',type=Path,required=True);add.add_argument('--bin-name',default='pio-standalone')
    remove=commands.add_parser('uninstall');remove.add_argument('--prefix',type=Path,required=True)
    job=commands.add_parser('render-job')
    for name in ('prefix','config','data-dir','socket','output'):job.add_argument('--'+name,type=Path,required=True)
    job.add_argument('--label',required=True);job.add_argument('--kind',choices=['launchd','systemd'],required=True)
    args=parser.parse_args()
    try:
        if args.command=='install':result=install(args.binary,args.prefix,args.bin_dir,args.bin_name)
        elif args.command=='uninstall':result=uninstall(args.prefix)
        else:
            exclusive(args.output,render(args.prefix,args.config,args.data_dir,args.socket,args.label,args.kind));result=dict(output=str(args.output),kind=args.kind,source='labeled fake service')
        print(json.dumps(result))
    except (OSError,ValueError,KeyError) as error:
        print(str(error),file=sys.stderr);sys.exit(2)
if __name__=='__main__':main()
