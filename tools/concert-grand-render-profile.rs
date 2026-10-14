//! Export the exact Concert + explicit tuning profile used by native renders.
//! Arguments: tuning.txt state.bin. Prints every public parameter as JSON.
use rackforge_concert_grand::{ConcertGrand, apply_tuning};

fn main() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let args: Vec<String> = std::env::args().collect();
            assert_eq!(args.len(), 3);
            let mut piano = Box::new(ConcertGrand::default());
            assert!(piano.load_preset("concert"));
            let (_, faders, complaints) = apply_tuning(&std::fs::read_to_string(&args[1]).unwrap());
            assert!(complaints.is_empty(), "{complaints:?}");
            for (index, value) in faders {
                assert!(piano.set_parameter(index as u32, value as f64));
            }
            assert!(piano.prepare(44_100.0, 256, 0, 2));
            let mut out = [0.0f32; 512];
            for at in (0..8820).step_by(256) {
                let n = (8820 - at).min(256);
                piano.process(&[], &mut out[..2 * n], &[], &[], n as u32, 0, 2);
            }
            let mut state = [0u8; 16_384];
            let size = piano.save_state(&mut state).unwrap();
            std::fs::write(&args[2], &state[..size]).unwrap();
            print!("{{\"sample_rate\":44100,\"block\":256,\"values\":[");
            let mut count = 0;
            // Public controls leave index 199 unused; the preamp lives at 200.
            for index in 0..=200 {
                let Some(value) = piano.get_parameter(index) else {
                    continue;
                };
                assert!(value.is_finite());
                if count > 0 {
                    print!(",");
                }
                print!("{{\"index\":{index},\"value\":{value}}}");
                count += 1;
            }
            println!("],\"parameter_count\":{count},\"state_bytes\":{size}}}");
        })
        .unwrap()
        .join()
        .unwrap();
}
