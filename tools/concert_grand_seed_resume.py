"""Authenticate a completed contact checkpoint against its committed producer."""
import hashlib
import json
import re
import subprocess
from pathlib import Path
from concert_grand_wave_cache import digest
from concert_grand_contact_seed import load_seed

PRODUCER='audit-concert-grand-coast-512.py'


def authenticated_seed(report_path,seed_path,commit,expected,source_directory=None):
    if not re.fullmatch('[0-9a-f]{40}',commit):raise ValueError('full producer commit required')
    report_path=Path(report_path);seed_path=Path(seed_path)
    old=json.loads(report_path.read_text());tools=source_directory or Path(__file__).parent
    if (old.get('stage')!='independent complete angular certificate' or old.get('all_gates_passed') is not False
            or old.get('rates')!=[128000,256000,512000] or len(old.get('cases',[]))!=3
            or old.get('contact_rate')!=3072000 or old.get('duration_s')!=.0005
            or not old.get('contact_seed',{}).get('roundtrip_exact')):
        raise ValueError('completed physical checkpoint and exact seed required')
    for key in ['board_sha256','cache_R_sha256','geometry','microphones','parent_sha256','criteria']:
        if old[key]!=expected[key]:raise ValueError('changed checkpoint binding: '+key)
    for name,sha in old['parent_sha256'].items():
        if digest(name)!=sha:raise ValueError('changed checkpoint parent: '+name)
    blob=subprocess.run(['git','show',commit+':tools/'+PRODUCER],check=True,capture_output=True).stdout
    # Git may normalize text to LF while checkout uses CRLF. Permit only those
    # two standard byte representations of this exact committed producer.
    lf=blob.replace(b'\r\n',b'\n')
    producer_hashes={hashlib.sha256(blob).hexdigest(),hashlib.sha256(lf).hexdigest(),
                     hashlib.sha256(lf.replace(b'\n',b'\r\n')).hexdigest()}
    if old['source_sha256'].get(PRODUCER) not in producer_hashes:
        raise ValueError('checkpoint producer differs from committed code')
    for name,sha in old['source_sha256'].items():
        if name!=PRODUCER and digest(tools/name)!=sha:raise ValueError('changed physical/observation source: '+name)
    if (Path(old['contact_seed']['path']).resolve()!=seed_path.resolve()
            or digest(seed_path)!=old['contact_seed']['payload_sha256']
            or digest(seed_path.with_suffix('.json'))!=old['contact_seed']['manifest_sha256']):
        raise ValueError('changed canonical seed identity')
    reproduction=[old['baseline_reproduction'],*old['coast_reproduction'].values()]
    if max(v for row in reproduction for group in row.values() for v in group.values())>=old['criteria']['seed_relative']:
        raise ValueError('checkpoint did not reproduce its measured parent')
    bindings=dict(source_sha256=old['source_sha256'],parent_sha256=old['parent_sha256'],
        board_sha256=old['board_sha256'],cache_R_sha256=old['cache_R_sha256'],geometry=old['geometry'],
        contact_rate=old['contact_rate'],contact_duration_s=.003,incident_speed_m_s=5.)
    state,identity=load_seed(seed_path,bindings)
    return state,old,dict(checkpoint_sha256=digest(report_path),producer_commit=commit,
                         historical_producer_sha256=old['source_sha256'][PRODUCER],**identity)
