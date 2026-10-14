"""Verify the production library against the published CI audio fingerprint.

CI starts default() without a preset reload or diagnostic warmup. A temporary
caller uses exactly that initialization; the measured caller stays unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-dir',type=Path,required=True);p.add_argument('--work',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True);a=p.parse_args()
    if a.work.exists() or a.report.exists():raise ValueError('fresh paths required')
    a.work.mkdir(parents=True);root=Path(__file__).resolve().parents[1]
    original=root/'tools/concert-grand-release-probe.rs';source=original.read_text(encoding='utf-8')
    changes=[('assert!(piano.load_preset(&args[6]));','// CI uses untouched default().'),
        ('let warmup = (rate as usize) / 5;','let warmup = 0;')]
    for before,after in changes:
        assert source.count(before)==1;source=source.replace(before,after)
    caller=a.work/'fingerprint-probe.rs';caller.write_text(source,encoding='utf-8',newline='\n')
    exe=a.work/'fingerprint-probe.exe';build=a.build_dir.resolve()
    compile_command=['rustc','+1.98.0-x86_64-pc-windows-msvc','--edition=2024','-O','-L',f'dependency={build}',
        '--extern',f'rackforge_plugin_sdk={build}/librackforge_plugin_sdk.rlib',
        '--extern',f'rackforge_concert_grand={build}/librackforge_concert_grand.rlib',str(caller),'-o',str(exe)]
    subprocess.run(compile_command,check=True)
    events=[(0,176,64,127)]
    for block in range(0,240,20):
        events.extend((128+block*128+i*7,144,note+block//20,96) for i,note in enumerate([36,43,48,55,60]))
    event_path=a.work/'events.txt';event_path.write_text(''.join(' '.join(map(str,e))+'\n' for e in events),encoding='utf-8',newline='\n')
    raw=a.work/'audio.f32';command=[str(exe.resolve()),str(event_path.resolve()),str(raw.resolve()),'48000',str(241*128),'128','concert']
    native=json.loads(subprocess.check_output(command,text=True))
    h=0xcbf29ce484222325
    for (word,) in struct.iter_unpack('<I',raw.read_bytes()[128*2*4:]):h=((h^word)*0x100000001b3)&0xffffffffffffffff
    assert h==0xe74df30116b39a5a,hex(h)
    report=dict(passed=True,expected='e74df30116b39a5a',actual=f'{h:016x}',native=native,
        initialization_scope=__doc__,compile_command=compile_command,command=command,
        verifier_sha256=digest(__file__),measured_caller_sha256=digest(original),oracle_caller_path=str(caller.resolve()),
        oracle_caller_sha256=digest(caller),production_library_sha256=digest(build/'librackforge_concert_grand.rlib'),
        sdk_library_sha256=digest(build/'librackforge_plugin_sdk.rlib'),raw_sha256=digest(raw),source_changes=changes)
    a.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8',newline='\n')
    print('Production library matches published fingerprint:',report['actual'])


if __name__=='__main__':main()
