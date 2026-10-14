"""Add a test-only observer to a frozen source copy; trace Nocturne voices.

The production tree remains untouched. The observer must reproduce the entire
53-second public-probe render byte for byte before its state data are used.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np


OBSERVER = r'''
    #[test]
    #[ignore = "offline context diagnosis"]
    fn export_nocturne_context() {
        use std::io::Write;
        let base = std::path::PathBuf::from(std::env::var("CG_CONTEXT_DIR").unwrap());
        let text = std::fs::read_to_string(base.join("original-0-53s.txt")).unwrap();
        let events: Vec<(usize, [u8; 3])> = text.lines().map(|line| {
            let a: Vec<usize> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
            (a[0], [a[1] as u8, a[2] as u8, a[3] as u8])
        }).collect();
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.load_preset("concert"));
        const RATE: usize = 44100;
        const BLOCK: usize = 256;
        assert!(piano.prepare(RATE as f64, BLOCK as u32, 0, 2));
        let mut out = vec![0.0; 2*BLOCK];
        for at in (0..RATE/5).step_by(BLOCK) {
            let n = BLOCK.min(RATE/5-at);
            piano.process(&[], &mut out[..2*n], &[], &[], n as u32, 0, 2);
        }
        let create = |name: &str| std::io::BufWriter::new(std::fs::OpenOptions::new()
            .write(true).create_new(true).open(base.join(name)).unwrap());
        let mut raw = create("context.f32");
        let mut csv = create("context.csv");
        writeln!(csv, "frame,slot,note,held,sustained,onset,push_in,partials,noise_amp,thump_amp,damper_phase,damper_applied,energy,h1_norm,h1_projection,h1_rc,h1_rs,h1_drain,h1_push").unwrap();
        let mut next = 0;
        for at in (0..53*RATE).step_by(BLOCK) {
            let n = BLOCK.min(53*RATE-at);
            let mut midi = Vec::new();
            while next < events.len() && events[next].0 < at+n {
                let (frame, data) = events[next];
                midi.push(MidiEvent {frame:(frame-at) as u32,data,length:3});
                next += 1;
            }
            piano.process(&[], &mut out[..2*n], &midi, &[], n as u32, 0, 2);
            for sample in &out[..2*n] {raw.write_all(&sample.to_le_bytes()).unwrap();}
            if at+n < 47*RATE || at > 52*RATE {continue;}
            for (slot,v) in piano.voices().enumerate() {
                if !v.active || ![45,46,53,58,60,62,65,77,79].contains(&v.note) {continue;}
                let p = v.partials[..v.partial_count].iter().find(|p| (p.slope.abs()-0.0625).abs() < 1e-6);
                let (mut norm,mut sum_s,mut sum_c,mut push) = (0.0f32,0.0f32,0.0f32,0.0f32);
                if let Some(p) = p {
                    for lane in 0..=LANE_HORIZONTAL {
                        norm += p.s[lane]*p.s[lane]+p.c[lane]*p.c[lane];
                        sum_s += p.s[lane];sum_c += p.c[lane];
                        push += p.push[lane]*p.push[lane]+p.push_s[lane]*p.push_s[lane];
                    }
                }
                let (rc,rs,drain) = p.map(|p|(p.rc[0],p.rs[0],p.drain)).unwrap_or_default();
                writeln!(csv, "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    at+n,slot,v.note,v.held as u8,v.sustained as u8,v.onset,v.push_in,v.partial_count,
                    v.noise_amp,v.thump_amp,v.damper_phase,v.damper_applied,v.energy,
                    norm.sqrt(),(sum_s*sum_s+sum_c*sum_c).sqrt(),rc,rs,drain,push.sqrt()).unwrap();
            }
        }
        raw.flush().unwrap();csv.flush().unwrap();
    }
'''


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root=Path(__file__).resolve().parents[1]
    work=root/'local/concert-grand-nocturne-attacks-2026-10-14'
    build=work/'observer';build.mkdir(exist_ok=False)
    docs=root/'docs/concert-grand-release-body-2026-10-14'
    public=json.loads((docs/'string-tension-verification.json').read_text(encoding='utf-8'))
    source_path=root/'plugins/concert-grand/src/lib.rs'
    assert sha(source_path)==public['source_sha256']
    source=source_path.read_text(encoding='utf-8')
    marker='    #[test]\n    #[ignore = "offline body diagnosis; CG_BODY_TRACE_DIR must be a fresh directory"]'
    assert source.count(marker)==1
    observed=source.replace(marker,OBSERVER+'\n'+marker)
    for filename in ('presets.json','parameters.json'):
        observed=observed.replace(f'include_str!("../package/metadata/{filename}")',
            'include_str!(r"'+str(root/'plugins/concert-grand/package/metadata'/filename)+'")')
    path=build/'lib.rs';path.write_text(observed,encoding='utf-8',newline='\n')
    (build/'math.rs').write_bytes((root/'plugins/concert-grand/src/math.rs').read_bytes())
    sdk=next(p for p in public['files_sha256'] if p.endswith('librackforge_plugin_sdk.rlib'))
    exe=build/'tests.exe'
    command=['rustc','+1.98.0-x86_64-pc-windows-msvc','--edition=2024','--test','-O','-L',f'dependency={Path(sdk).parent}',
             '--extern',f'rackforge_plugin_sdk={sdk}',str(path),'-o',str(exe)]
    subprocess.run(command,cwd=root,check=True)
    env=dict(os.environ,RUST_MIN_STACK='33554432',CG_CONTEXT_DIR=str(work))
    run=[str(exe),'--exact','tests::export_nocturne_context','--ignored','--nocapture','--test-threads=1']
    result=subprocess.run(run,cwd=root,env=env,capture_output=True,text=True)
    assert result.returncode==0,result.stdout+result.stderr
    assert sha(work/'context.f32')==sha(work/'original.f32'), 'Observer must not change the listened-to rendering'
    assert sha(source_path)==public['source_sha256']
    report=dict(scope=__doc__,production_source_sha256=sha(source_path),observer_source_sha256=sha(path),
        generator_sha256=sha(__file__),compile_command=command,test_command=run,test_output=result.stdout,
        context_audio_bit_identical_to_public_probe=True,raw_sha256=sha(work/'context.f32'),
        trace_path=str(work/'context.csv'),trace_sha256=sha(work/'context.csv'),
        observer_source_path=str(path),test_executable_sha256=sha(exe),
        limitations='Block-end observations every 256 samples; internal modal norms are not SI energy. No changes to production synthesis.')
    (docs/'nocturne-context.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8',newline='\n')
    print('Context observer is byte-identical to all 53 seconds of public production audio',flush=True)


if __name__=='__main__':main()
