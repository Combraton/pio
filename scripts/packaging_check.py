#!/usr/bin/env python3
"""Exercise collision-safe layout and real per-user service manager start/stop."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile
from public_api import Client, CREDENTIAL
from fake_host_matrix import poll

ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,required=True);args=parser.parse_args();args.out.mkdir(parents=True,exist_ok=False)
    root=Path(tempfile.mkdtemp(prefix='pio-pkg-',dir='/tmp')).resolve()
    commands=[];report=dict(format='pio-packaging-check/1',source='fake-host/process',real_adapter=False,head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),platform=platform.platform(),root=str(root),commands=commands)
    def run(argv, expected=0, env=None):
        result=subprocess.run(list(map(str,argv)),capture_output=True,text=True,timeout=20,env=env)
        commands.append(dict(argv=list(map(str,argv)),exit=result.returncode,stdout=result.stdout,stderr=result.stderr))
        if expected is not None:assert result.returncode==expected,commands[-1]
        return result
    def package(*argv,**kw):return run(['python3',ROOT/'scripts/package.py',*argv],**kw)
    stop=None;unit=None
    try:
        prefix=root/'private install';bins=root/'bin';bins.mkdir();sentinel=bins/'pio';sentinel.write_text('#!/bin/sh\necho unrelated-pio\n');sentinel.chmod(0o755)
        witness=lambda:dict(inode=sentinel.stat().st_ino,sha256=hashlib.sha256(sentinel.read_bytes()).hexdigest())
        before=witness();env=dict(os.environ,PATH=str(bins)+os.pathsep+os.environ['PATH'])
        collision=package('install','--binary',ROOT/'target/debug/pio','--prefix',prefix,'--bin-dir',bins,'--bin-name','pio',expected=2,env=env)
        assert 'placement_collision' in collision.stderr and not prefix.exists() and witness()==before
        installed=package('install','--binary',ROOT/'target/debug/pio','--prefix',prefix,'--bin-dir',bins,env=env)
        manifest=json.loads(installed.stdout);(args.out/'install.json').write_text(json.dumps(manifest,indent=2)+'\n')
        package('install','--binary',ROOT/'target/debug/pio','--prefix',prefix,'--bin-dir',bins,expected=2,env=env)
        data=root/'state';data.mkdir(mode=0o700);socket=data/'pio.sock';config=root/'service.json'
        config.write_text(json.dumps(dict(format='pio-fake-service/1',protocol=dict(format='combraton-conformance-config/1',principal='owner',credentials=[dict(credential=CREDENTIAL)]),fake_host=dict(duration_ms=1000,fault=''))));config.chmod(0o600)
        label='io.combraton.pio.ci.'+str(os.getpid());kind='launchd' if platform.system()=='Darwin' else 'systemd'
        job=args.out/('service.plist' if kind=='launchd' else 'service.service')
        package('render-job','--prefix',prefix,'--config',config,'--data-dir',data,'--socket',socket,'--output',job,'--label',label,'--kind',kind)
        if kind=='launchd':
            domain=f'gui/{os.getuid()}'
            run(['launchctl','print',domain])
            target=f'{domain}/{label}';stop=['launchctl','bootout',target]
            run(['plutil','-lint',job]);run(['launchctl','bootstrap',domain,job.resolve()])
            state=poll(lambda:run(['launchctl','print',target]),lambda r:re.search(r'\bpid = (\d+)',r.stdout))
            pid=int(re.search(r'\bpid = (\d+)',state.stdout).group(1))
        else:
            runtime=Path(os.environ.setdefault('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))
            report['manager_environment']=dict(XDG_RUNTIME_DIR=str(runtime),runtime_owner=runtime.stat().st_uid,runtime_mode=oct(runtime.stat().st_mode & 0o777))
            run(['systemctl','--user','--version'])
            run(['systemctl','--user','show-environment'])
            directory=runtime/'systemd/user';directory.mkdir(parents=True,exist_ok=True);unit=directory/(label+'.service')
            with unit.open('xb') as f:f.write(job.read_bytes())
            stop=['systemctl','--user','stop',unit.name]
            run(['systemctl','--user','daemon-reload']);run(['systemctl','--user','start',unit.name]);run(['systemctl','--user','is-active','--quiet',unit.name])
            pid=int(run(['systemctl','--user','show',unit.name,'--property=MainPID','--value']).stdout.strip())
        def ready():
            with Client(socket,args.out/'transcript.jsonl') as client:return client.query('execution.discovery.list',{})
        discovery=poll(ready,lambda r:'result' in r)
        identity=json.loads(run([prefix/'bin/pio','fake','identity',pid]).stdout)
        assert 'fake' in discovery['result']['installations'][0]['harness']
        run(stop);stop=None
        poll(lambda:subprocess.run([str(prefix/'bin/pio'),'fake','identity',str(pid)],capture_output=True).returncode,lambda rc:rc!=0)
        if kind=='launchd':assert run(['launchctl','print',target],expected=None).returncode!=0
        else:run(['systemctl','--user','is-active','--quiet',unit.name],expected=3)
        run([ROOT/'target/debug/pio','check-transcript',args.out/'transcript.jsonl'])
        # Manifest tampering must stop uninstall before any deletion.
        manifest_path=prefix/'install.json';original_manifest=manifest_path.read_bytes()
        manifest_path.write_bytes(original_manifest+b' ')
        tampering=package('uninstall','--prefix',prefix,expected=2)
        assert 'modified_owned_file: manifest' in tampering.stderr and (bins/'pio-standalone').is_symlink() and (prefix/'bin/pio').exists()
        manifest_path.write_bytes(original_manifest)
        # Modified owned binary must stop uninstall before any deletion.
        payload=prefix/'bin/pio';original=payload.read_bytes();payload.write_bytes(original+b'changed')
        refusal=package('uninstall','--prefix',prefix,expected=2);assert 'modified_owned_file' in refusal.stderr and (bins/'pio-standalone').is_symlink()
        payload.write_bytes(original)
        preserved=prefix/'user-note';preserved.write_text('preserve unowned addition')
        removed=package('uninstall','--prefix',prefix)
        assert preserved.read_text()=='preserve unowned addition' and (data/'journal.sqlite3').exists()
        assert not (prefix/'install.json').exists() and not payload.exists() and not os.path.lexists(bins/'pio-standalone') and witness()==before
        report.update(outcome='pass',manager=kind,started_identity=identity,stop_observed=True,discovery=discovery,unrelated_pio_before=before,unrelated_pio_after=witness(),collision_refusal=collision.stderr,modified_removal_refusal=refusal.stderr,manifest_tampering_refusal=tampering.stderr,temporary_state='retained for diagnosis outside repository',uninstall=json.loads(removed.stdout),state_preserved=True)
    except Exception as error:
        report.update(outcome='fail',reason=f'{type(error).__name__}: {error}');raise
    finally:
        if stop:run(stop,expected=None)
        if unit and unit.exists():unit.unlink();run(['systemctl','--user','daemon-reload'],expected=None)
        (args.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('commands','discovery')},indent=2))
if __name__=='__main__':main()
