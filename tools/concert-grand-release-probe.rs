//! Native caller of the unchanged production piano for offline diagnosis.
//! Arguments: events.txt output.f32 rate frames block preset [tuning.txt].
//! Events are absolute sample frames: frame status data1 data2, in stable order.
use rackforge_concert_grand::ConcertGrand;
use rackforge_plugin_sdk::MidiEvent;
use std::{fs, io::{BufWriter, Write}, time::Instant};

fn run() {
    let args: Vec<String> = std::env::args().collect();
    assert!((7..=8).contains(&args.len()));
    let rate: u32 = args[3].parse().unwrap();
    let total: usize = args[4].parse().unwrap();
    let block: usize = args[5].parse().unwrap();
    assert!((8000..=192000).contains(&rate) && (1..=512).contains(&block) && total > 0);
    let events: Vec<(usize, [u8; 3])> = fs::read_to_string(&args[1]).unwrap().lines().map(|line| {
        let a: Vec<usize> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
        assert!(a.len() == 4 && a[0] < total && a[1] <= 255 && a[2] <= 127 && a[3] <= 127);
        (a[0], [a[1] as u8, a[2] as u8, a[3] as u8])
    }).collect();
    assert!(events.windows(2).all(|e| e[0].0 <= e[1].0));
    let mut piano = Box::new(ConcertGrand::default());
    assert!(piano.load_preset(&args[6]));
    if args.len() == 8 {
        let text = fs::read_to_string(&args[7]).unwrap();
        let (_, faders, complaints) = rackforge_concert_grand::apply_tuning(&text);
        assert!(complaints.is_empty(), "{complaints:?}");
        for (index, value) in faders { assert!(piano.set_parameter(index.try_into().unwrap(), value as f64)); }
    }
    assert!(piano.prepare(rate as f64, block as u32, 0, 2));
    let mut output = vec![0f32; 2 * block];
    let warmup = (rate as usize) / 5;
    for at in (0..warmup).step_by(block) {
        let n = block.min(warmup - at);
        piano.process(&[], &mut output[..2*n], &[], &[], n as u32, 0, 2);
    }
    let file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2]).unwrap();
    let mut sink = BufWriter::new(file);
    let mut midi = Vec::new();
    let mut next = 0;
    let mut peak = 0f32;
    let mut energy = 0f64;
    let begin = Instant::now();
    for at in (0..total).step_by(block) {
        let n = block.min(total - at);
        midi.clear();
        while next < events.len() && events[next].0 < at + n {
            let (frame, data) = events[next];
            assert!(frame >= at);
            midi.push(MidiEvent { frame: (frame - at) as u32, data, length: 3 });
            next += 1;
        }
        piano.process(&[], &mut output[..2*n], &midi, &[], n as u32, 0, 2);
        for &sample in &output[..2*n] {
            assert!(sample.is_finite());
            peak = peak.max(sample.abs());
            energy += (sample as f64).powi(2);
            sink.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
    sink.flush().unwrap();
    assert_eq!(next, events.len());
    println!("{{\"frames\":{total},\"rate\":{rate},\"block\":{block},\"warmup_frames\":{warmup},\"events\":{next},\"peak\":{peak},\"rms\":{},\"wall_seconds\":{}}}",
        (energy / (2 * total) as f64).sqrt(), begin.elapsed().as_secs_f64());
}

fn main() {
    std::thread::Builder::new().stack_size(32 * 1024 * 1024).spawn(run).unwrap().join().unwrap();
}
