"""Bake an audition's effective coefficients into the portable piano source.

This is an explicit release step; it does not change installed packages.
Keep the calibration JSON with the source to record the selected coefficients.
"""
import argparse
import json
from pathlib import Path
import re
import struct


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--profile', type=Path, required=True)
    p.add_argument('--plugin', type=Path, required=True)
    a = p.parse_args()
    profile = json.loads(a.profile.read_text(encoding='utf-8'))
    # Promote onto the canonical portable source, retaining its wasm fixes,
    # state migration and mechanism tests. Research snapshots are not copied.
    source = (a.plugin/'src/lib.rs').read_text(encoding='utf-8')
    for name, value in profile['tuning'].items():
        source, count = re.subn(r'(pub static '+re.escape(name)+r': Knob = Knob::new\()[^;]+?(\);)',
                               lambda m: m[1]+repr(float(value))+m[2], source)
        assert count == 1, (name, count)
    start = source.index('impl Default for Controls')
    end = source.index('impl Controls', start)
    defaults = source[start:end]
    for name, value in profile['controls'].items():
        defaults, count = re.subn(r'(\b'+re.escape(name)+r': )[^,]+,',
                                 lambda m: m[1]+repr(float(value))+',', defaults)
        assert count == 1, (name, count)
    source = source[:start]+defaults+source[end:]
    # The audition gain is downstream of the microphone preamp. Moving it
    # upstream would change the transformer's nonlinearity and the timbre.
    source,count = re.subn(r'(pub static OUTPUT_CALIBRATION_GAIN: Knob = Knob::new\()[^;]+?(\);)',
        lambda m:m[1]+repr(float(profile['output_gain']))+m[2],source)
    assert count==1
    (a.plugin/'src/lib.rs').write_text(source,encoding='utf-8')
    manifest = a.plugin/'package/rackforge-plugin.toml'
    manifest.write_text(re.sub(r'^version = "[^"]+"', 'version = "'+profile['version']+'"',
                       manifest.read_text(encoding='utf-8'), flags=re.M), encoding='utf-8')
    runtime_path = a.plugin/'package/metadata/runtime.json'
    runtime = json.loads(runtime_path.read_text(encoding='utf-8'))
    runtime['version'] = profile['version']
    runtime_path.write_text(json.dumps(runtime,indent=2)+'\n',encoding='utf-8')
    schema_path = a.plugin/'package/metadata/parameters.json'
    schema = json.loads(schema_path.read_text(encoding='utf-8'))
    compiled = {name: f32(float(value.replace('_',''))) for name,value in
                re.findall(r'pub static (\w+): Knob = Knob::new\(([-+0-9.eE_]+)\);',source)}
    for param in schema['parameters']:
        kind = param['kind']
        if param['name'] in compiled:
            value = compiled[param['name']]
            kind['default'] = value
            kind['minimum'],kind['maximum'] = (0.0,1.0) if value == 0 else tuple(sorted((value/16,value*16)))
            if value == 0:
                kind.pop('taper',None)
        if str(param['index']) in profile['params']:
            kind['default'] = f32(profile['params'][str(param['index'])])
        if param['index'] == 31:
            kind['minimum'] = f32(f32(compiled['BOARD_LOSS_FACTOR']*100)/4)
            kind['maximum'] = f32(f32(compiled['BOARD_LOSS_FACTOR']*100)*4)
    schema_path.write_text(json.dumps(schema,indent=2)+'\n',encoding='utf-8')
    print('Promoted', profile['version'], len(profile['tuning']), 'coefficients', flush=True)


if __name__ == '__main__':
    main()
