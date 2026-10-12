"""Same authenticated vector experiment with eight independent SIMD reduction chains."""
import hashlib
import json
from pathlib import Path
import sys


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


if __name__ == '__main__':
    root = Path(__file__).resolve().parent.parent
    parent = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12/vector.json'
    reference = Path(__file__).with_name('distill-concert-grand-vector-body.py')
    expected = json.loads(parent.read_text())['source_sha256'][reference.name]
    if digest(reference) != expected:
        raise ValueError('frozen vector experiment changed')
    source = reference.read_text()
    replacements = {
        "docs/'reciproco.json'": "docs/'avx2.json'",
        'concert-grand-vector-body.rs': 'concert-grand-ilp-body.rs',
        'native-vector.rs': 'native-ilp.rs',
        'vector-body-bench.exe': 'ilp-body-bench.exe',
        'vector-body-': 'ilp-body-',
        'mode-parallel reciprocal dry body measured': 'parallel-sum reciprocal dry body measured',
    }
    for before, after in replacements.items():
        if before not in source:
            raise ValueError('experiment transformation marker missing: '+before)
        source = source.replace(before, after)
    generated_sha = hashlib.sha256(source.encode()).hexdigest()
    context = {'__name__': 'authenticated_avx2_experiment', '__file__': __file__}
    exec(compile(source, str(reference), 'exec'), context)
    context['main']()
    report_path = Path(sys.argv[sys.argv.index('--report')+1])
    report = json.loads(report_path.read_text())
    report.update(generated_producer_sha256=generated_sha,
        frozen_vector_producer_sha256=expected,
        experiment_changes='Only native kernel/source paths and parent report; identical coefficients, physical model, gates, input pattern, rates and timing protocol.')
    report_path.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n', encoding='utf-8')

