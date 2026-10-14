"""Observe the actual F5 bridge input, preserving the full Nocturne render."""
import hashlib
import json
import os
from pathlib import Path
import subprocess


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root=Path(__file__).resolve().parents[1]
    work=root/'local/concert-grand-nocturne-attacks-2026-10-14'
    docs=root/'docs/concert-grand-release-body-2026-10-14'
    old=json.loads((docs/'nocturne-context.json').read_text(encoding='utf-8'))
    source=Path(old['observer_source_path']);assert sha(source)==old['observer_source_sha256']
    production=root/'plugins/concert-grand/src/lib.rs';assert sha(production)==old['production_source_sha256']
    build=work/'melody-observer';build.mkdir(exist_ok=False)
    s=source.read_text(encoding='utf-8')
    marker='        let force = sample * (self.pan_left + self.pan_right);'
    assert s.count(marker)==1
    s=s.replace(marker,marker+'''
        #[cfg(test)]
        if self.note == 77 {
            tests::NOCT_VOICE_TAP.with(|tap| tap.borrow_mut().push([sample, force, self.keybed_out]));
        }
''')
    marker='    #[test]\n    #[ignore = "offline context diagnosis"]'
    assert s.count(marker)==1
    s=s.replace(marker,'''    std::thread_local! {
        pub(super) static NOCT_VOICE_TAP: std::cell::RefCell<Vec<[f32;3]>> = std::cell::RefCell::new(Vec::new());
    }
'''+marker)
    s=s.replace('create("context.f32")','create("melody-context.f32")').replace('create("context.csv")','create("melody-context.csv")')
    s=s.replace('let mut csv = create("melody-context.csv");', 'let mut csv = create("melody-context.csv");\n        let mut tap_raw = create("melody-force.f32");')
    marker='            piano.process(&[], &mut out[..2*n], &midi, &[], n as u32, 0, 2);'
    assert s.count(marker)==1
    s=s.replace(marker,'''            NOCT_VOICE_TAP.with(|tap| tap.borrow_mut().clear());
'''+marker+'''
            NOCT_VOICE_TAP.with(|tap| {
                let tap = tap.borrow();
                if at >= 49*RATE && at+n <= 51*RATE+RATE/2 {assert_eq!(tap.len(), n);}
                for frame in 0..n {
                    let sample = if tap.len()==n {tap[frame]} else {[0.0;3]};
                    for channel in sample {tap_raw.write_all(&channel.to_le_bytes()).unwrap();}
                }
            });
''')
    s=s.replace('raw.flush().unwrap();csv.flush().unwrap();','raw.flush().unwrap();csv.flush().unwrap();tap_raw.flush().unwrap();')
    path=build/'lib.rs';path.write_text(s,encoding='utf-8',newline='\n')
    (build/'math.rs').write_bytes((root/'plugins/concert-grand/src/math.rs').read_bytes())
    public=json.loads((docs/'string-tension-verification.json').read_text(encoding='utf-8'))
    sdk=next(p for p in public['files_sha256'] if p.endswith('librackforge_plugin_sdk.rlib'))
    exe=build/'tests.exe'
    command=['rustc','+1.98.0-x86_64-pc-windows-msvc','--edition=2024','--test','-O','-L',f'dependency={Path(sdk).parent}',
             '--extern',f'rackforge_plugin_sdk={sdk}',str(path),'-o',str(exe)]
    subprocess.run(command,cwd=root,check=True)
    run=[str(exe),'--exact','tests::export_nocturne_context','--ignored','--nocapture','--test-threads=1']
    result=subprocess.run(run,cwd=root,env=dict(os.environ,RUST_MIN_STACK='33554432',CG_CONTEXT_DIR=str(work)),capture_output=True,text=True)
    assert result.returncode==0,result.stdout+result.stderr
    assert sha(work/'melody-context.f32')==sha(work/'original.f32')
    assert sha(production)==old['production_source_sha256']
    report=dict(scope=__doc__,generator_sha256=sha(__file__),production_source_sha256=sha(production),
        observer_source_sha256=sha(path),observer_source_path=str(path),compile_command=command,test_command=run,
        test_output=result.stdout,full_audio_identical_to_public_probe=True,raw_sha256=sha(work/'melody-context.f32'),
        tap_path=str(work/'melody-force.f32'),tap_sha256=sha(work/'melody-force.f32'),
        tap_columns=['voice_sample','bridge_force','keybed_out'],tap_channels=3,rate=44100,
        validated_continuous_tap_seconds=[49,51.5],
        limitations='Tap is in internal model units before the soundboard and microphone transfer. It is not an acoustic solo stem or a calibrated sound pressure.')
    (docs/'nocturne-melody.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8',newline='\n')
    print('F5 force observer preserves all 53 seconds byte for byte',flush=True)


if __name__=='__main__':main()
