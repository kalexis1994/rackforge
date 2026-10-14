//! Linux-only diagnostic: installed portable library, serial or four worker
//! instances, exact renderer events and warmup. No audio-device changes.
//! Args: library events output.f32 frames serial|parallel preset [fixed-blocks].
//! fixed-blocks prepares and processes only complete 256-frame periods, then
//! trims the final file to the requested duration. Default preserves the
//! reference renderer's 200 ms warmup, including its short final warmup block.
use rackforge_plugin_sdk::portable::native::{EntryFnV1, NativeApiV1, region};
use std::{
    ffi::{CString, c_char, c_void},
    fs,
    io::{BufWriter, Write},
    sync::mpsc,
    time::Instant,
};
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
struct Instance {
    api: &'static NativeApiV1,
    at: *mut c_void,
}
// The portable native ABI supports moving an exclusively owned instance to
// an audio worker. No two threads call the same instance concurrently.
unsafe impl Send for Instance {}
impl Instance {
    fn new(api: &'static NativeApiV1, preset: &str, worker: bool) -> Self {
        unsafe {
            let mut s = Self {
                api,
                at: (api.create)(),
            };
            assert!(!s.at.is_null());
            assert_eq!((api.initialize)(s.at), 0);
            if worker {
                assert_eq!((api.prepare)(s.at, 44100.0, 256, 0, 2), 0);
            }
            s.buf::<u8>(region::TRANSFER)[..preset.len()].copy_from_slice(preset.as_bytes());
            assert_eq!((api.load_preset)(s.at, preset.len() as i32), 0);
            assert_eq!((api.set_parameter)(s.at, 192, 0.004_f32 as f64), 0);
            if !worker {
                assert_eq!((api.prepare)(s.at, 44100.0, 256, 0, 2), 0);
            }
            assert_eq!((api.get_parameter)(s.at, 192), 0.004_f32 as f64);
            s
        }
    }
    fn buf<T>(&mut self, id: u32) -> &mut [T] {
        unsafe {
            let mut n = 0;
            let p = (self.api.region)(self.at, id, &mut n);
            assert!(!p.is_null());
            std::slice::from_raw_parts_mut(p.cast(), n)
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            (self.api.destroy)(self.at);
        }
    }
}
struct Job {
    frames: usize,
    payload: Vec<u8>,
    shared: Vec<u8>,
}
struct Renderer {
    main: Instance,
    workers: Vec<(
        mpsc::Sender<Option<Job>>,
        mpsc::Receiver<(Vec<f32>, Vec<u8>)>,
        std::thread::JoinHandle<()>,
    )>,
}
impl Renderer {
    fn block(&mut self, n: usize, midi: &[u64]) -> Vec<f32> {
        let api = self.main.api;
        self.main.buf::<u64>(region::MIDI)[..midi.len()].copy_from_slice(midi);
        unsafe {
            if self.workers.is_empty() {
                assert_eq!(
                    (api.process)(self.main.at, n as i32, 0, 2, midi.len() as i32, 0),
                    0
                );
            } else {
                let par = api.parallel.unwrap();
                let count = (par.begin_block)(self.main.at, n as i32, 0, 2, midi.len() as i32, 0);
                assert!(count >= 0 && count <= par.max_units as i32);
                let plan = self.main.buf::<u32>(region::PLAN)[..2 + 2 * count as usize].to_vec();
                let shared = self.main.buf::<u8>(region::SHARED)[..plan[0] as usize].to_vec();
                for e in plan[2..].chunks_exact(2) {
                    let u = e[0] as usize;
                    let at = u * par.dispatch_stride as usize;
                    let payload =
                        self.main.buf::<u8>(region::DISPATCH)[at..at + e[1] as usize].to_vec();
                    self.workers[u]
                        .0
                        .send(Some(Job {
                            frames: n,
                            payload,
                            shared: shared.clone(),
                        }))
                        .unwrap();
                }
                for e in plan[2..].chunks_exact(2) {
                    let u = e[0] as usize;
                    let (out, report) = self.workers[u].1.recv().unwrap();
                    let at = u * par.mix_slot_samples as usize;
                    self.main.buf::<f32>(region::MIX)[at..at + out.len()].copy_from_slice(&out);
                    if !report.is_empty() {
                        let at = u * par.report_stride as usize;
                        self.main.buf::<u8>(region::REPORTS)[at..at + report.len()]
                            .copy_from_slice(&report);
                    }
                }
                assert_eq!((par.end_block)(self.main.at, n as i32, 2), 0);
            }
        }
        self.main.buf::<f32>(region::OUTPUT)[..2 * n].to_vec()
    }
}
fn run() {
    let a: Vec<String> = std::env::args().collect();
    assert!(a.len() == 7 || a.len() == 8);
    let fixed_blocks = a.len() == 8;
    if fixed_blocks {
        assert_eq!(a[7], "fixed-blocks");
    }
    let api = unsafe {
        let handle = dlopen(CString::new(a[1].as_str()).unwrap().as_ptr(), 2);
        assert!(!handle.is_null());
        let symbol = dlsym(handle, c"rackforge_portable_native_entry_v1".as_ptr());
        assert!(!symbol.is_null());
        &*std::mem::transmute::<*mut c_void, EntryFnV1>(symbol)()
    };
    let mut r = Renderer {
        main: Instance::new(api, &a[6], false),
        workers: Vec::new(),
    };
    assert!(a[5] == "serial" || a[5] == "parallel");
    if a[5] == "parallel" {
        let par = api.parallel.unwrap();
        for u in 0..par.max_units as usize {
            let mut unit = Instance::new(api, &a[6], true);
            let (tx, rx) = mpsc::channel::<Option<Job>>();
            let (answer_tx, answer_rx) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    while let Some(job) = rx.recv().unwrap() {
                        let at = u * par.dispatch_stride as usize;
                        unit.buf::<u8>(region::DISPATCH)[at..at + job.payload.len()]
                            .copy_from_slice(&job.payload);
                        unit.buf::<u8>(region::SHARED)[..job.shared.len()]
                            .copy_from_slice(&job.shared);
                        unsafe {
                            assert_eq!(
                                (par.render_unit)(
                                    unit.at,
                                    u as i32,
                                    job.payload.len() as i32,
                                    job.shared.len() as i32,
                                    job.frames as i32,
                                    2
                                ),
                                0
                            );
                        }
                        let out = unit.buf::<f32>(region::OUTPUT)
                            [..job.frames * par.unit_channels as usize]
                            .to_vec();
                        let report = if par.report_stride > 0 {
                            let at = u * par.report_stride as usize;
                            unit.buf::<u8>(region::REPORTS)[at..at + par.report_stride as usize]
                                .to_vec()
                        } else {
                            vec![]
                        };
                        answer_tx.send((out, report)).unwrap();
                    }
                })
                .unwrap();
            r.workers.push((tx, answer_rx, thread));
        }
    }
    let total: usize = a[4].parse().unwrap();
    let events: Vec<Vec<usize>> = fs::read_to_string(&a[2])
        .unwrap()
        .lines()
        .map(|l| l.split_whitespace().map(|w| w.parse().unwrap()).collect())
        .collect();
    assert!(events.iter().all(|e| e.len() == 4));
    assert!(events.windows(2).all(|p| p[0][0] <= p[1][0]));
    let warmup_frames = if fixed_blocks { 35 * 256 } else { 8820 };
    for at in (0..warmup_frames).step_by(256) {
        r.block(256.min(warmup_frames - at), &[]);
    }
    let mut file = BufWriter::new(
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&a[3])
            .unwrap(),
    );
    let mut next = 0;
    let start = Instant::now();
    for at in (0..total).step_by(256) {
        let written_frames = 256.min(total - at);
        let n = if fixed_blocks { 256 } else { written_frames };
        let mut midi = Vec::new();
        while next < events.len() && events[next][0] < at + n {
            let e = &events[next];
            assert!(e[0] >= at);
            midi.push(
                (e[0] - at) as u64
                    | (e[1] as u64) << 32
                    | (e[2] as u64) << 40
                    | (e[3] as u64) << 48
                    | 3_u64 << 56,
            );
            next += 1;
        }
        for sample in r.block(n, &midi).into_iter().take(2 * written_frames) {
            assert!(sample.is_finite());
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
    file.flush().unwrap();
    println!(
        "{{\"mode\":\"{}\",\"frames\":{total},\"events\":{next},\"warmup_frames\":{warmup_frames},\"fixed_blocks\":{fixed_blocks},\"wall_seconds\":{}}}",
        a[5],
        start.elapsed().as_secs_f64()
    );
    for (tx, _, thread) in r.workers.drain(..) {
        tx.send(None).unwrap();
        thread.join().unwrap();
    }
}
fn main() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
