"""Verify final test goldens without repeating the unchanged acoustic sweep.

The acoustic audit ran before updating its two expected test fingerprints.
Prove that those test constants/comments are the only subsequent source edits,
then rebuild, test, compile WASM, and check a native render byte for byte.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root = Path(__file__).resolve().parents[1]
    work = root/'local/concert-grand-string-tension-2026-10-14'
    docs = root/'docs/concert-grand-release-body-2026-10-14'
    audit_path = docs/'string-tension.json'
    audit = json.loads(audit_path.read_text(encoding='utf-8'))
    source_path = root/'plugins/concert-grand/src/lib.rs'
    measured_path = work/'measured-lib.rs'
    assert digest(measured_path) == audit['files_sha256'][str(source_path)]
    measured = measured_path.read_text(encoding='utf-8')
    final = source_path.read_text(encoding='utf-8')
    expected = measured.replace('0x581f_8087_da14_bcbf', '0xe4ab_cfd0_2a57_57c1').replace(
        '0x3f40_537e_871f_c7c3', '0xdfb7_f193_9622_35e0').replace(
        '// The common mechanical port now uses its own normalization, excludes\n'
        '    // absent strings and retains quiet coupled coordinates. Both scripts\n'
        '    // change globally; no note-specific tuning correction is present.',
        '// Separate string/polarisation slopes are squared before summing the\n'
        '    // scalar tension estimate. Both scripts intentionally change; the bridge\n'
        '    // port and all note-dependent calibration remain as in the accepted base.')
    assert final == expected, 'Only the explicitly listed test edits may follow the acoustic audit'
    marker = '#[cfg(test)]\nmod bench {'
    assert measured.split(marker)[0] == final.split(marker)[0]
    for path, sha in audit['files_sha256'].items():
        if Path(path) != source_path:
            assert digest(path) == sha, path
    build = work/'final-build'
    build.mkdir(exist_ok=False)
    env = dict(os.environ, RUST_MIN_STACK='33554432')

    def run(command, label):
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
        log = work/f'{label}.log'
        log.write_text(result.stdout+result.stderr, encoding='utf-8', newline='\n')
        assert result.returncode == 0, f'{label}: {result.stdout}{result.stderr}'
        print(label, 'passed', flush=True)
        return dict(command=command, log_path=str(log), log_sha256=digest(log), passed=True)

    old_build = str(Path(audit['compile_commands'][0][-1]).parent)
    commands = [[arg.replace(old_build, str(build)) for arg in command] for command in audit['compile_commands']]
    compiles = [run(command, f'final-native-{i}') for i, command in enumerate(commands)]
    sdk = next(path for path in audit['files_sha256'] if path.endswith('librackforge_plugin_sdk.rlib'))
    rust = ['rustc', '+1.98.0-x86_64-pc-windows-msvc', '--edition=2024', '-O']
    tests = build/'tests.exe'
    test_compile = run([*rust, '--test', '-L', f'dependency={Path(sdk).parent}', '--extern',
                        f'rackforge_plugin_sdk={sdk}', str(source_path), '-o', str(tests)], 'final-test-compile')
    suite = run([str(tests), '--test-threads=1'], 'final-native-tests')
    assert '82 passed; 0 failed; 69 ignored' in Path(suite['log_path']).read_text(encoding='utf-8')
    fingerprints = [run([str(tests), '--exact', name, '--ignored', '--nocapture', '--test-threads=1'],
                        f'final-fingerprint-{i}') for i, name in enumerate(
                        ['bench::render_fingerprint', 'bench::render_fingerprint_pedals'])]
    raw = build/'benchmark.f32'
    command = [str(build/'probe.exe'), str(root/'local/concert-grand-top-unison-final-2026-10-14/benchmark.txt'),
               str(raw), '44100', str(14*44100), '256', 'concert']
    render = run(command, 'final-native-equivalence')
    prior_raw = work/'audit/bench-0-B.f32'
    assert digest(raw) == digest(prior_raw), 'Test-only edits must leave native audio byte-identical'
    wasm_sdk, wasm = build/'librackforge_plugin_sdk_wasm.rlib', build/'concert-grand.wasm'
    wasm_commands = [
        [*rust, '--target', 'wasm32-unknown-unknown', '-C', 'target-feature=+simd128', '--crate-name',
         'rackforge_plugin_sdk', '--crate-type', 'rlib', str(root/'crates/rackforge-plugin-sdk/src/lib.rs'), '-o', str(wasm_sdk)],
        [*rust, '--target', 'wasm32-unknown-unknown', '-C', 'target-feature=+simd128', '-C', 'link-arg=-zstack-size=8388608',
         '--crate-name', 'rackforge_concert_grand', '--crate-type', 'cdylib', '--extern',
         f'rackforge_plugin_sdk={wasm_sdk}', str(source_path), '-o', str(wasm)],
    ]
    wasm_results = [run(command, f'final-wasm-{i}') for i, command in enumerate(wasm_commands)]
    files = [source_path, root/'plugins/concert-grand/src/math.rs', root/'crates/rackforge-plugin-sdk/src/lib.rs',
             root/'crates/rackforge-plugin-sdk/src/portable.rs', root/'tools/concert-grand-release-probe.rs',
             Path(__file__), Path(sdk), build/'probe.exe', build/'librackforge_concert_grand.rlib', tests, wasm]
    report = dict(source_sha256=digest(source_path), measured_source_sha256=digest(measured_path),
        acoustic_audit_sha256=digest(audit_path), production_source_unchanged_after_acoustic_audit=True,
        post_audit_source_edits='Only two expected fingerprints and their explanatory comment in cfg(test) mod bench',
        native_compile=compiles, test_compile=test_compile, native_tests=suite, fingerprints=fingerprints,
        counts=dict(passed=82, failed=0, ignored=69), full_render=render,
        native_audio_equivalence=dict(prior_raw=str(prior_raw), final_raw=str(raw), sha256=digest(raw), identical=True),
        wasm_compile=wasm_results, wasm_runtime_tested=False,
        physical_change=audit['physical_change'], files_sha256={str(path):digest(path) for path in files})
    (docs/'string-tension-verification.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8',newline='\n')


if __name__ == '__main__':
    main()
