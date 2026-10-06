//! Native builds of portable processors.
//!
//! A package may carry, beside its `wasm-v1` component, the same processor
//! built for the host's platform. The SDK exports both from one source, and
//! the native build answers through a table of the component's own entries
//! (`rackforge_plugin_sdk::portable::native`), each taking the instance
//! first. This backend drives that table with the component's calling
//! sequence and the component's checks, so a caller cannot tell which of the
//! two it holds -- except that a native build is not sandboxed and has no
//! fuel. A build whose processor renders in units carries the parallel-render
//! table too, and its instances are scheduled exactly as a component's are.
//! Which packages are trusted to run one is the caller's decision, not this
//! module's.
//!
//! Two threads, by the work. The real-time entries -- `process`, parameters,
//! `reset`, latency, the parallel stages -- run on the caller's thread, which may be an audio
//! thread that cannot wait on another. Everything else runs on the module's
//! control thread, whose stack is as large as a component's: a processor
//! built by value can need megabytes while it is made and prepared (RF-
//! Musette's engine is ~1.1 MiB and moved several times, and links its
//! component with an 8 MiB stack for it), which a host's own threads -- a
//! DAW's among them -- do not promise.

use crate::shared::{
    PROGRAM_EDIT_BASIC, PROGRAM_EDIT_DECLARATIVE, PROGRAM_EDIT_KNOWN_CAPABILITIES,
    PROGRAM_EDIT_PREVIEW, check_status, checked_samples, validate_parallel_plan,
    validate_realtime_events,
};
use crate::{
    ABI_VERSION_V1, ABI_VERSION_V1_1, MAX_PARALLEL_UNITS, MidiEvent, MidiEvent2,
    PARALLEL_ABI_VERSION_V1, ParallelBlockPlan, ParallelLayout, ParallelPlanEntry, ParameterEvent,
};

use anyhow::{Context, Result, bail};
use rackforge_plugin_sdk::portable::native::{
    ENTRY_SYMBOL_V1, EntryFnV1, LengthFn, NATIVE_ABI_VERSION_V1, NativeApiV1, NativeParallelApiV1,
    region,
};
use std::ffi::c_void;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The control thread's stack: reserved, not committed, on every platform
/// RackForge runs on, so the size costs address space rather than memory.
/// Eight times the largest stack a shipped component links with.
const CONTROL_STACK_BYTES: usize = 64 << 20;

/// Calls on a thread with a stack the processor can be built on.
struct ControlThread {
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    thread: Option<JoinHandle<()>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ControlThread {
    fn start() -> Result<Self> {
        let (jobs, queue) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("rf-native-ctl".to_owned())
            .stack_size(CONTROL_STACK_BYTES)
            .spawn(move || {
                for job in queue {
                    job();
                }
            })
            .context("starting a native plugin's control thread")?;
        Ok(Self {
            jobs: Mutex::new(Some(jobs)),
            thread: Some(thread),
        })
    }

    /// Runs `work` on the control thread and waits for it. A panic in it is
    /// carried back to the caller.
    fn run<R: Send>(&self, work: impl FnOnce() -> R + Send) -> R {
        let (done, result) = mpsc::sync_channel(1);
        let job: Box<dyn FnOnce() + Send + '_> = Box::new(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
            let _ = done.send(outcome);
        });
        // SAFETY: the job borrows from this frame, and this frame does not
        // return until the job has run or been dropped: `recv` below waits
        // for its result or for `done` to be dropped with it, and a job the
        // channel refuses comes back here and runs before anything returns.
        let job: Job = unsafe { std::mem::transmute(job) };
        let refused = match self
            .jobs
            .lock()
            .ok()
            .as_ref()
            .and_then(|jobs| jobs.as_ref())
        {
            Some(jobs) => jobs.send(job).err().map(|refused| refused.0),
            None => Some(job),
        };
        // No thread to take it: the caller's own, which is what the call
        // would have had without one.
        if let Some(job) = refused {
            job();
        }
        match result.recv() {
            Ok(Ok(value)) => value,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => unreachable!("a control job ends by sending its result"),
        }
    }
}

impl Drop for ControlThread {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.take();
        }
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

/// A loaded native build: its table, the library that holds it, and the
/// thread its control calls run on.
pub(crate) struct LibraryModule {
    api: *const NativeApiV1,
    // Declared before the library so it is joined before the code it may
    // have been running is unmapped.
    control: ControlThread,
    _library: Option<libloading::Library>,
}

// SAFETY: the table is immutable, static data of the library this module
// keeps loaded, and the contract lets any one thread at a time call an
// instance's entries.
unsafe impl Send for LibraryModule {}
unsafe impl Sync for LibraryModule {}

impl LibraryModule {
    /// Loads the native build at `path` and checks its table.
    ///
    /// # Safety
    ///
    /// Loading runs the library's initialisers, and every later call runs
    /// its code in this process, unsandboxed: the caller vouches for it.
    pub(crate) unsafe fn open(path: &Path) -> Result<Self> {
        // Absolute, so the system loads this file and no other: on Windows a
        // relative name is looked up along the library search path instead.
        let path = &path
            .canonicalize()
            .with_context(|| format!("resolving native plugin build {}", path.display()))?;
        let library = unsafe { libloading::Library::new(path) }
            .with_context(|| format!("loading native plugin build {}", path.display()))?;
        let entry = *unsafe { library.get::<EntryFnV1>(ENTRY_SYMBOL_V1) }.with_context(|| {
            format!(
                "native plugin build {} does not export the portable entry",
                path.display()
            )
        })?;
        unsafe { Self::from_entry(entry, Some(library)) }
    }

    /// A native build already linked into this process, by its entry.
    ///
    /// # Safety
    ///
    /// As [`Self::open`]; `library`, when given, is the one `entry` is in.
    pub(crate) unsafe fn from_entry(
        entry: EntryFnV1,
        library: Option<libloading::Library>,
    ) -> Result<Self> {
        let api = unsafe { entry() };
        if api.is_null() {
            bail!("native plugin build returned no portable table");
        }
        // SAFETY: non-null, and static for as long as the library is loaded.
        let table = unsafe { &*api };
        // A table only ever grows, so a build may be newer than this host;
        // never older than the fields read here.
        if (table.struct_size as usize) < size_of::<NativeApiV1>() {
            bail!(
                "native plugin table is {} bytes, smaller than the {} this host reads",
                table.struct_size,
                size_of::<NativeApiV1>()
            );
        }
        if table.native_abi_version >> 16 != NATIVE_ABI_VERSION_V1 >> 16 {
            bail!(
                "unsupported native plugin table version {:#010x}",
                table.native_abi_version
            );
        }
        let version = i32::try_from(table.portable_abi_version).unwrap_or(-1);
        if !(ABI_VERSION_V1_1..=ABI_VERSION_V1).contains(&version) {
            bail!("unsupported wasm-v1 ABI version {version:#010x} in a native build");
        }
        Ok(Self {
            api,
            control: ControlThread::start()?,
            _library: library,
        })
    }

    fn api(&self) -> &NativeApiV1 {
        // SAFETY: checked non-null at load, and kept loaded by `self`.
        unsafe { &*self.api }
    }
}

/// An instance pointer, carried to the control thread.
#[derive(Clone, Copy)]
struct Handle(*mut c_void);

// SAFETY: an instance is used by one thread at a time; `&mut self` on every
// call that reaches it, and a control call blocks its caller until it ends.
unsafe impl Send for Handle {}

impl Handle {
    fn get(self) -> *mut c_void {
        self.0
    }
}

/// An instance made but not yet accepted: destroyed if it is refused.
struct Created<'module> {
    module: &'module LibraryModule,
    handle: Handle,
}

impl Drop for Created<'_> {
    fn drop(&mut self) {
        let module = self.module;
        let handle = self.handle;
        module
            .control
            .run(move || unsafe { (module.api().destroy)(handle.get()) });
    }
}

/// One of an instance's regions: its address and capacity in elements.
struct Region<T> {
    at: *mut T,
    capacity: usize,
}

impl<T> Region<T> {
    /// The region as the instance's `region` entry describes it, checked the
    /// way a component's pointers are: present and aligned, and not empty
    /// unless it is audio a processor may take none of, as a component may
    /// declare an audio capacity of zero.
    fn of(
        api: &NativeApiV1,
        instance: Handle,
        which: u32,
        name: &str,
        may_be_empty: bool,
    ) -> Result<Self> {
        let mut capacity = 0;
        // SAFETY: `instance` is live; the entry only reads it.
        let at = unsafe { (api.region)(instance.get(), which, &mut capacity) }.cast::<T>();
        if capacity == 0 && may_be_empty {
            return Ok(Self {
                at: std::ptr::NonNull::dangling().as_ptr(),
                capacity: 0,
            });
        }
        if at.is_null() || capacity == 0 {
            bail!("native plugin build has no {name} region");
        }
        if !at.is_aligned() {
            bail!("native plugin build returned a misaligned {name} region");
        }
        Ok(Self { at, capacity })
    }

    /// SAFETY: the instance is live and not running: the region is ours.
    unsafe fn slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.at, self.capacity) }
    }

    /// SAFETY: as [`Self::slice`].
    unsafe fn slice_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.at, self.capacity) }
    }
}

pub(crate) struct LibraryInstance {
    module: Arc<LibraryModule>,
    instance: Handle,
    input: Region<f32>,
    output: Region<f32>,
    midi: Region<u64>,
    parameters: Region<rackforge_plugin_sdk::ParameterEvent>,
    transfer: Region<u8>,
    exchange: Region<u8>,
    /// The wide-MIDI region, two words an event: present exactly when the
    /// build has the wide entry, as a component's four exports are.
    midi2: Option<Region<u64>>,
    capabilities: u32,
    /// The parallel-render extension, for a build whose processor renders
    /// in units.
    parallel: Option<NativeParallel>,
    prepared_input_channels: u32,
    prepared_output_channels: u32,
    maximum_frames: u32,
}

/// A native build's parallel-render table and its instance's regions, which
/// the instance makes on first use; read once it has been initialized.
struct NativeParallel {
    api: &'static NativeParallelApiV1,
    layout: ParallelLayout,
    dispatch: Region<u8>,
    plan: Region<u32>,
    mix: Region<f32>,
    shared: Region<u8>,
    reports: Option<Region<u8>>,
}

// SAFETY: see `Handle`; the regions belong to the instance and are touched
// only through `&mut self` or, for reads, `&self` between calls.
unsafe impl Send for LibraryInstance {}
unsafe impl Sync for LibraryInstance {}

impl LibraryInstance {
    pub(crate) fn new(module: Arc<LibraryModule>) -> Result<Self> {
        let control_module = Arc::clone(&module);
        let created = module.control.run(move || {
            // SAFETY: the table's own constructor.
            Handle(unsafe { (control_module.api().create)() })
        });
        if created.get().is_null() {
            bail!("native plugin build could not create an instance");
        }
        // Destroyed again if it is refused before it is an instance.
        let guard = Created {
            module: &module,
            handle: created,
        };
        let api = module.api();
        let input = Region::of(api, created, region::INPUT, "input", true)?;
        let output = Region::of(api, created, region::OUTPUT, "output", true)?;
        let midi = Region::of(api, created, region::MIDI, "MIDI event", false)?;
        let parameters = Region::of(api, created, region::PARAMETERS, "parameter event", false)?;
        let transfer = Region::<u8>::of(api, created, region::TRANSFER, "transfer", false)?;
        let exchange =
            Region::<u8>::of(api, created, region::EXCHANGE_INPUT, "program input", false)?;
        if exchange.capacity < transfer.capacity {
            bail!("native plugin build's program input is smaller than its transfer region");
        }
        let (transfer_start, exchange_start) = (transfer.at as usize, exchange.at as usize);
        if transfer_start < exchange_start + exchange.capacity
            && exchange_start < transfer_start + transfer.capacity
        {
            bail!("portable plugin program input and output buffers must not overlap");
        }
        let midi2 = match api.process_v2 {
            Some(_) => {
                let wide =
                    Region::<u64>::of(api, created, region::MIDI2, "wide MIDI event", false)?;
                if wide.capacity < 2 {
                    bail!("component reported a non-positive wide-MIDI capacity");
                }
                Some(wide)
            }
            None => None,
        };
        std::mem::forget(guard);
        // From here an error destroys the instance through `Drop`.
        let mut instance = Self {
            module,
            instance: created,
            input,
            output,
            midi,
            parameters,
            transfer,
            exchange,
            midi2,
            capabilities: 0,
            parallel: None,
            prepared_input_channels: 0,
            prepared_output_channels: 0,
            maximum_frames: 0,
        };
        check_status(
            instance.control(|api, at| unsafe { (api.initialize)(at) }),
            "initialize",
        )?;
        let capabilities =
            instance.control(|api, at| unsafe { (api.program_editing_capabilities)(at) });
        if capabilities < 0 {
            bail!("portable plugin returned invalid program-editing capabilities");
        }
        let capabilities = capabilities as u32;
        if capabilities & !PROGRAM_EDIT_KNOWN_CAPABILITIES != 0
            || capabilities != 0 && capabilities & PROGRAM_EDIT_BASIC == 0
        {
            bail!(
                "portable plugin returned unsupported program-editing capabilities {capabilities:#x}"
            );
        }
        instance.capabilities = capabilities;
        instance.parallel = instance.parallel_table()?;
        Ok(instance)
    }

    /// One call on the control thread.
    fn control<R: Send>(&self, call: impl FnOnce(&NativeApiV1, *mut c_void) -> R + Send) -> R {
        let module = &*self.module;
        let instance = self.instance;
        module
            .control
            .run(move || call(module.api(), instance.get()))
    }

    /// One call on the caller's thread: the real-time entries.
    fn direct<R>(&mut self, call: impl FnOnce(&NativeApiV1, *mut c_void) -> R) -> R {
        call(self.module.api(), self.instance.get())
    }

    pub(crate) fn load_resource_file(&mut self, id: &str, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("opening portable resource {}", path.display()))?;
        let total_bytes = file
            .metadata()
            .with_context(|| format!("reading portable resource metadata {}", path.display()))?
            .len();
        self.begin_resource(id, total_bytes)?;
        let mut reader = BufReader::new(file);
        let mut chunk = vec![0_u8; self.transfer.capacity.min(64 * 1024)];
        let mut offset = 0_u64;
        loop {
            let read = reader
                .read(&mut chunk)
                .with_context(|| format!("reading portable resource {}", path.display()))?;
            if read == 0 {
                break;
            }
            self.write_resource(offset, &chunk[..read])?;
            offset += read as u64;
        }
        if offset != total_bytes {
            bail!("portable resource changed while it was being delivered");
        }
        self.end_resource()
    }

    pub(crate) fn prepare(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        let input_samples = checked_samples(maximum_frames, input_channels)?;
        let output_samples = checked_samples(maximum_frames, output_channels)?;
        if input_samples > self.input.capacity {
            bail!(
                "plugin input capacity {} samples is smaller than requested {input_samples}",
                self.input.capacity
            );
        }
        if output_samples > self.output.capacity {
            bail!(
                "plugin output capacity {} samples is smaller than requested {output_samples}",
                self.output.capacity
            );
        }
        check_status(
            self.control(|api, at| unsafe {
                (api.prepare)(
                    at,
                    sample_rate,
                    maximum_frames as i32,
                    input_channels as i32,
                    output_channels as i32,
                )
            }),
            "prepare",
        )?;
        self.prepared_input_channels = input_channels;
        self.prepared_output_channels = output_channels;
        self.maximum_frames = maximum_frames;
        Ok(())
    }

    pub(crate) fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        check_status(
            self.direct(|api, at| unsafe { (api.set_parameter)(at, index as i32, value) }),
            "set_parameter",
        )
    }

    pub(crate) fn get_parameter(&mut self, index: u32) -> Result<f64> {
        let value = self.direct(|api, at| unsafe { (api.get_parameter)(at, index as i32) });
        if !value.is_finite() {
            bail!("portable plugin does not expose parameter {index}");
        }
        Ok(value)
    }

    pub(crate) fn latency_frames(&mut self) -> Result<u32> {
        let value = self.direct(|api, at| unsafe { (api.latency_frames)(at) });
        if value < 0 {
            bail!("portable plugin returned an invalid latency");
        }
        Ok(value as u32)
    }

    pub(crate) fn reset(&mut self) -> Result<()> {
        check_status(self.direct(|api, at| unsafe { (api.reset)(at) }), "reset")
    }

    pub(crate) fn load_resource(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        self.begin_resource(id, bytes.len() as u64)?;
        for (chunk_index, chunk) in bytes.chunks(self.transfer.capacity).enumerate() {
            let offset = chunk_index
                .checked_mul(self.transfer.capacity)
                .context("resource offset overflow")? as u64;
            self.write_resource(offset, chunk)?;
        }
        self.end_resource()
    }

    fn begin_resource(&mut self, id: &str, total_bytes: u64) -> Result<()> {
        if id.is_empty() || id.len() > self.transfer.capacity {
            bail!("resource id does not fit the portable transfer buffer");
        }
        let total_bytes = i64::try_from(total_bytes).context("resource is too large")?;
        self.write_transfer(id.as_bytes())?;
        let length = id.len() as i32;
        check_status(
            self.control(|api, at| unsafe { (api.resource_begin)(at, length, total_bytes) }),
            "resource_begin",
        )
    }

    fn write_resource(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.transfer.capacity {
            bail!("resource chunk does not fit the portable transfer buffer");
        }
        let offset = i64::try_from(offset).context("resource offset is too large")?;
        self.write_transfer(bytes)?;
        let length = bytes.len() as i32;
        check_status(
            self.control(|api, at| unsafe { (api.resource_write)(at, offset, length) }),
            "resource_write",
        )
    }

    fn end_resource(&mut self) -> Result<()> {
        check_status(
            self.control(|api, at| unsafe { (api.resource_end)(at) }),
            "resource_end",
        )
    }

    pub(crate) fn preset_catalog(&mut self) -> Result<Option<Vec<u8>>> {
        let length = self.control(|api, at| unsafe { (api.preset_catalog)(at) });
        if length == 0 {
            return Ok(None);
        }
        if length < 0 || length as usize > self.transfer.capacity {
            bail!("portable plugin returned an invalid preset catalog length {length}");
        }
        Ok(Some(self.read_transfer(length as usize)))
    }

    pub(crate) fn load_preset(&mut self, id: &str) -> Result<()> {
        if id.is_empty() {
            bail!("preset id must not be empty");
        }
        self.write_transfer(id.as_bytes())?;
        let length = id.len() as i32;
        check_status(
            self.control(|api, at| unsafe { (api.load_preset)(at, length) }),
            "load_preset",
        )
    }

    pub(crate) fn save_state(&mut self) -> Result<Vec<u8>> {
        let length = self.control(|api, at| unsafe { (api.save_state)(at) });
        if length < 0 || length as usize > self.transfer.capacity {
            bail!("portable plugin returned an invalid state length {length}");
        }
        Ok(self.read_transfer(length as usize))
    }

    pub(crate) fn load_state(&mut self, state: &[u8]) -> Result<()> {
        self.write_transfer(state)?;
        let length = state.len() as i32;
        check_status(
            self.control(|api, at| unsafe { (api.load_state)(at, length) }),
            "load_state",
        )
    }

    pub(crate) fn program_editing_capabilities(&self) -> u32 {
        self.capabilities
    }

    fn require_program_editing(&self) -> Result<()> {
        if self.capabilities == 0 {
            bail!("portable plugin does not expose program editing");
        }
        Ok(())
    }

    fn require_declarative(&self, missing: &str) -> Result<()> {
        self.require_program_editing()?;
        if self.capabilities & PROGRAM_EDIT_DECLARATIVE == 0 {
            bail!("{missing}");
        }
        Ok(())
    }

    pub(crate) fn begin_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.require_program_editing()?;
        self.exchange_program_bytes(request, "begin_program_edit", |api| api.program_begin_edit)
    }

    pub(crate) fn prepare_program_save(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        self.require_program_editing()?;
        self.exchange_program_bytes(document, "prepare_program_save", |api| {
            api.program_prepare_save
        })
    }

    pub(crate) fn install_program(&mut self, prepared: &[u8]) -> Result<()> {
        self.require_program_editing()?;
        self.call_program_install(prepared, "install_program", |api| api.program_install)
    }

    pub(crate) fn preview_program(&mut self, prepared: &[u8]) -> Result<bool> {
        self.require_program_editing()?;
        if self.capabilities & PROGRAM_EDIT_PREVIEW == 0 {
            return Ok(false);
        }
        self.call_program_install(prepared, "preview_program", |api| api.program_preview)?;
        Ok(true)
    }

    pub(crate) fn program_editor_view(&mut self, document: &[u8]) -> Result<Vec<u8>> {
        self.require_declarative("portable plugin does not expose a declarative program editor")?;
        self.exchange_program_bytes(document, "program_editor_view", |api| {
            api.program_editor_view
        })
    }

    pub(crate) fn apply_program_edit(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        self.require_declarative("portable plugin does not expose declarative program edits")?;
        self.exchange_program_bytes(request, "apply_program_edit", |api| api.program_apply_edit)
    }

    fn exchange_program_bytes(
        &mut self,
        source: &[u8],
        operation: &str,
        entry: fn(&NativeApiV1) -> LengthFn,
    ) -> Result<Vec<u8>> {
        self.write_program_input(source)?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        let length = self.control(|api, at| unsafe { entry(api)(at, source_length) });
        if length < 0 || length as usize > self.transfer.capacity {
            bail!("portable plugin {operation} returned invalid length {length}");
        }
        Ok(self.read_transfer(length as usize))
    }

    fn call_program_install(
        &mut self,
        source: &[u8],
        operation: &str,
        entry: fn(&NativeApiV1) -> LengthFn,
    ) -> Result<()> {
        self.write_program_input(source)?;
        let source_length = i32::try_from(source.len()).context("program payload is too large")?;
        check_status(
            self.control(|api, at| unsafe { entry(api)(at, source_length) }),
            operation,
        )
    }

    fn write_program_input(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.transfer.capacity {
            bail!("program payload exceeds plugin transfer capacity");
        }
        // SAFETY: the instance is not running; the region is ours.
        let exchange = unsafe { self.exchange.slice_mut() };
        exchange[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn write_transfer(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.transfer.capacity {
            bail!("control payload exceeds plugin transfer capacity");
        }
        // SAFETY: as above.
        let transfer = unsafe { self.transfer.slice_mut() };
        transfer[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn read_transfer(&self, length: usize) -> Vec<u8> {
        // SAFETY: as above; the caller checked `length` against the capacity.
        let transfer = unsafe { self.transfer.slice() };
        transfer[..length].to_vec()
    }

    pub(crate) fn midi2_families(&self) -> u32 {
        if self.midi2.is_some() {
            self.module.api().midi2_families
        } else {
            0
        }
    }

    pub(crate) fn process_interleaved_with_midi2(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
    ) -> Result<()> {
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if input.len() != input_samples || output.len() != output_samples {
            bail!("audio buffer length does not match prepared input/output channels");
        }
        validate_realtime_events(
            frames,
            midi,
            parameters,
            self.midi.capacity,
            self.parameters.capacity,
        )?;
        let capacity_midi2_events = self.midi2.as_ref().map_or(0, |wide| wide.capacity / 2);
        if midi2.len() > capacity_midi2_events {
            bail!("wide MIDI event count exceeds plugin capacity");
        }
        if midi2.iter().any(|event| event.frame >= frames) {
            bail!("wide MIDI event is outside the audio block");
        }
        // SAFETY: the instance is not running; the regions are ours, and
        // every count was checked against its capacity above.
        unsafe {
            self.input.slice_mut()[..input_samples].copy_from_slice(input);
            for (slot, event) in self.midi.slice_mut().iter_mut().zip(midi) {
                *slot = event.packed();
            }
            for (slot, event) in self.parameters.slice_mut().iter_mut().zip(parameters) {
                *slot = rackforge_plugin_sdk::ParameterEvent {
                    frame: event.frame,
                    index: event.index,
                    value: event.value,
                };
            }
            if let Some(wide) = &mut self.midi2 {
                for (slot, event) in wide
                    .slice_mut()
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(midi2)
                {
                    let (head, tail) = event.packed();
                    *slot = [head, tail];
                }
            }
        }
        let (input_channels, output_channels) = (
            self.prepared_input_channels as i32,
            self.prepared_output_channels as i32,
        );
        let (midi_count, parameter_count, midi2_count) = (
            midi.len() as i32,
            parameters.len() as i32,
            midi2.len() as i32,
        );
        let wide = self.midi2.is_some();
        let status = self.direct(|api, at| unsafe {
            match api.process_v2 {
                Some(process_v2) if wide => process_v2(
                    at,
                    frames as i32,
                    input_channels,
                    output_channels,
                    midi_count,
                    parameter_count,
                    midi2_count,
                ),
                _ => (api.process)(
                    at,
                    frames as i32,
                    input_channels,
                    output_channels,
                    midi_count,
                    parameter_count,
                ),
            }
        });
        check_status(status, "process")?;
        // SAFETY: as above.
        output.copy_from_slice(unsafe { &self.output.slice()[..output_samples] });
        Ok(())
    }
}

impl LibraryInstance {
    /// The parallel-render table and regions, checked as a component's
    /// parallel exports are.
    fn parallel_table(&self) -> Result<Option<NativeParallel>> {
        let api = self.module.api();
        // SAFETY: the table and what it points to are static data of the
        // library `self.module` keeps loaded.
        let Some(parallel) = api
            .parallel
            .map(|table| unsafe { &*(table as *const NativeParallelApiV1) })
        else {
            return Ok(None);
        };
        if (parallel.struct_size as usize) < size_of::<NativeParallelApiV1>() {
            bail!("native parallel-render table is smaller than this host reads");
        }
        if parallel.parallel_abi_version as i32 != PARALLEL_ABI_VERSION_V1 {
            bail!(
                "unsupported parallel-render ABI version {:#010x}",
                parallel.parallel_abi_version
            );
        }
        let max_units = parallel.max_units as usize;
        if !(1..=MAX_PARALLEL_UNITS).contains(&max_units) {
            bail!("parallel-render max_units {max_units} is outside 1..={MAX_PARALLEL_UNITS}");
        }
        let dispatch_stride = parallel.dispatch_stride as usize;
        if dispatch_stride == 0 || !dispatch_stride.is_multiple_of(8) {
            bail!("parallel-render dispatch stride must be a positive multiple of 8");
        }
        let shared_capacity = parallel.shared_capacity as usize;
        if shared_capacity == 0 || !shared_capacity.is_multiple_of(8) {
            bail!("parallel-render shared capacity must be a positive multiple of 8");
        }
        let report_stride = parallel.report_stride as usize;
        let mix_slot_samples = parallel.mix_slot_samples as usize;
        // A pre-stage that takes the wide count exactly when the block entry
        // does, as a component's exports must.
        match (api.process_v2.is_some(), parallel.begin_block_v2.is_some()) {
            (true, false) => bail!(
                "a build that takes MIDI 2.0 and renders in parallel must export the wide pre-stage"
            ),
            (false, true) => {
                bail!("a build exports the wide pre-stage without the wide-MIDI contract")
            }
            _ => {}
        }
        let instance = self.instance;
        let at_least = |name: &str, capacity: usize, needed: usize| -> Result<()> {
            if capacity < needed {
                bail!("native build's {name} region holds {capacity}, fewer than {needed}");
            }
            Ok(())
        };
        let dispatch = Region::<u8>::of(api, instance, region::DISPATCH, "dispatch", false)?;
        at_least("dispatch", dispatch.capacity, max_units * dispatch_stride)?;
        let plan = Region::<u32>::of(api, instance, region::PLAN, "plan", false)?;
        at_least("plan", plan.capacity, 2 + 2 * max_units)?;
        let mix = Region::<f32>::of(api, instance, region::MIX, "mix", false)?;
        at_least("mix", mix.capacity, max_units * mix_slot_samples)?;
        let shared = Region::<u8>::of(api, instance, region::SHARED, "shared", false)?;
        at_least("shared", shared.capacity, shared_capacity)?;
        let reports = if report_stride > 0 {
            let reports = Region::<u8>::of(api, instance, region::REPORTS, "report", false)?;
            at_least("report", reports.capacity, max_units * report_stride)?;
            Some(reports)
        } else {
            None
        };
        Ok(Some(NativeParallel {
            api: parallel,
            layout: ParallelLayout {
                max_units,
                dispatch_stride,
                mix_slot_samples,
                unit_channels: parallel.unit_channels as usize,
                report_stride,
                shared_capacity,
            },
            dispatch,
            plan,
            mix,
            shared,
            reports,
        }))
    }

    fn parallel(&self) -> Result<&NativeParallel> {
        self.parallel
            .as_ref()
            .context("portable plugin does not expose parallel render")
    }

    pub(crate) fn parallel_layout(&self) -> Option<ParallelLayout> {
        self.parallel.as_ref().map(|parallel| parallel.layout)
    }

    /// The serial pre-stage on a coordinator instance; see the Wasmtime
    /// backend's method of the same name, whose checks these are.
    pub(crate) fn parallel_begin_block(
        &mut self,
        input: &[f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
        plan: &mut [ParallelPlanEntry],
    ) -> Result<ParallelBlockPlan> {
        let layout = self.parallel()?.layout;
        if plan.len() < layout.max_units {
            bail!("parallel plan buffer is smaller than max_units");
        }
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        if input.len() != input_samples {
            bail!("audio buffer length does not match prepared input channels");
        }
        validate_realtime_events(
            frames,
            midi,
            parameters,
            self.midi.capacity,
            self.parameters.capacity,
        )?;
        let capacity_midi2_events = self.midi2.as_ref().map_or(0, |wide| wide.capacity / 2);
        if midi2.len() > capacity_midi2_events {
            bail!("wide MIDI event count exceeds plugin capacity");
        }
        if midi2.iter().any(|event| event.frame >= frames) {
            bail!("wide MIDI event is outside the audio block");
        }
        self.write_block_events(input, midi, parameters, midi2);
        let (input_channels, output_channels) = (
            self.prepared_input_channels as i32,
            self.prepared_output_channels as i32,
        );
        let (midi_count, parameter_count, midi2_count) = (
            midi.len() as i32,
            parameters.len() as i32,
            midi2.len() as i32,
        );
        let api = self.parallel()?.api;
        let active = self.direct(|_, at| unsafe {
            match api.begin_block_v2 {
                Some(begin_block_v2) => begin_block_v2(
                    at,
                    frames as i32,
                    input_channels,
                    output_channels,
                    midi_count,
                    parameter_count,
                    midi2_count,
                ),
                None => (api.begin_block)(
                    at,
                    frames as i32,
                    input_channels,
                    output_channels,
                    midi_count,
                    parameter_count,
                ),
            }
        });
        if active < 0 {
            bail!("portable plugin begin_block failed with status {active}");
        }
        let active = active as usize;
        if active > layout.max_units {
            bail!("portable plugin announced {active} units beyond max_units");
        }
        let parallel = self.parallel()?;
        // SAFETY: the instance is not running; the region is ours.
        let words = unsafe { parallel.plan.slice() };
        let shared_bytes = words[0] as usize;
        if shared_bytes > layout.shared_capacity {
            bail!(
                "portable plugin announced {shared_bytes} shared bytes beyond capacity {}",
                layout.shared_capacity
            );
        }
        for (entry, words) in plan[..active].iter_mut().zip(words[2..].as_chunks::<2>().0) {
            entry.unit = words[0];
            entry.payload_bytes = words[1];
        }
        validate_parallel_plan(&plan[..active], layout.max_units, layout.dispatch_stride)?;
        Ok(ParallelBlockPlan {
            active_units: active,
            shared_bytes,
        })
    }

    /// The block's input and events into their regions, already checked.
    fn write_block_events(
        &mut self,
        input: &[f32],
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        midi2: &[MidiEvent2],
    ) {
        // SAFETY: the instance is not running; the regions are ours, and
        // every count was checked against its capacity.
        unsafe {
            self.input.slice_mut()[..input.len()].copy_from_slice(input);
            for (slot, event) in self.midi.slice_mut().iter_mut().zip(midi) {
                *slot = event.packed();
            }
            for (slot, event) in self.parameters.slice_mut().iter_mut().zip(parameters) {
                *slot = rackforge_plugin_sdk::ParameterEvent {
                    frame: event.frame,
                    index: event.index,
                    value: event.value,
                };
            }
            if let Some(wide) = &mut self.midi2 {
                for (slot, event) in wide
                    .slice_mut()
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(midi2)
                {
                    let (head, tail) = event.packed();
                    *slot = [head, tail];
                }
            }
        }
    }

    pub(crate) fn parallel_read_shared(&self, shared: &mut [u8]) -> Result<()> {
        let parallel = self.parallel()?;
        if shared.len() > parallel.layout.shared_capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        // SAFETY: the instance is not running; the region is ours.
        shared.copy_from_slice(&unsafe { parallel.shared.slice() }[..shared.len()]);
        Ok(())
    }

    pub(crate) fn parallel_write_shared(&mut self, shared: &[u8]) -> Result<()> {
        let capacity = self.parallel()?.layout.shared_capacity;
        if shared.len() > capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        let region = &mut self.parallel.as_mut().expect("checked above").shared;
        // SAFETY: as above.
        let slice = unsafe { region.slice_mut() };
        slice[..shared.len()].copy_from_slice(shared);
        Ok(())
    }

    fn dispatch_at(&self, unit: u32, payload_bytes: usize) -> Result<usize> {
        let layout = self.parallel()?.layout;
        if unit as usize >= layout.max_units {
            bail!("parallel dispatch unit {unit} is beyond max_units");
        }
        if payload_bytes > layout.dispatch_stride {
            bail!("parallel dispatch payload exceeds the declared stride");
        }
        Ok(unit as usize * layout.dispatch_stride)
    }

    pub(crate) fn parallel_read_dispatch(&self, unit: u32, payload: &mut [u8]) -> Result<()> {
        let at = self.dispatch_at(unit, payload.len())?;
        // SAFETY: as above.
        let region = unsafe { self.parallel()?.dispatch.slice() };
        payload.copy_from_slice(&region[at..][..payload.len()]);
        Ok(())
    }

    pub(crate) fn parallel_write_dispatch(&mut self, unit: u32, payload: &[u8]) -> Result<()> {
        let at = self.dispatch_at(unit, payload.len())?;
        let region = &mut self.parallel.as_mut().expect("checked above").dispatch;
        // SAFETY: as above.
        let slice = unsafe { region.slice_mut() };
        slice[at..][..payload.len()].copy_from_slice(payload);
        Ok(())
    }

    /// Renders one unit inside a worker instance; its audio, the unit's own
    /// width, into `output`.
    pub(crate) fn parallel_render_unit(
        &mut self,
        unit: u32,
        payload_bytes: usize,
        shared_bytes: usize,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
    ) -> Result<()> {
        let parallel = self.parallel()?;
        let layout = parallel.layout;
        let api = parallel.api;
        if unit as usize >= layout.max_units {
            bail!("parallel render unit {unit} is beyond max_units");
        }
        if payload_bytes > layout.dispatch_stride {
            bail!("parallel dispatch payload exceeds the declared stride");
        }
        if shared_bytes > layout.shared_capacity {
            bail!("shared payload exceeds the declared capacity");
        }
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        let unit_width = layout.unit_width(self.prepared_output_channels as usize) as u32;
        let output_samples = checked_samples(frames, unit_width)?;
        if input.len() != input_samples || output.len() != output_samples {
            bail!("audio buffer length does not match the unit's width");
        }
        if output_samples > self.output.capacity {
            bail!("the unit's audio does not fit the output region");
        }
        // SAFETY: the instance is not running; the region is ours.
        let slice = unsafe { self.input.slice_mut() };
        slice[..input_samples].copy_from_slice(input);
        let output_channels = self.prepared_output_channels as i32;
        let status = self.direct(|_, at| unsafe {
            (api.render_unit)(
                at,
                unit as i32,
                payload_bytes as i32,
                shared_bytes as i32,
                frames as i32,
                output_channels,
            )
        });
        check_status(status, "parallel_render_unit")?;
        // SAFETY: as above.
        output.copy_from_slice(&unsafe { self.output.slice() }[..output_samples]);
        Ok(())
    }

    fn report_at(&self, unit: u32, len: usize) -> Result<usize> {
        let layout = self.parallel()?.layout;
        if unit as usize >= layout.max_units {
            bail!("parallel report unit {unit} is beyond max_units");
        }
        if len > layout.report_stride {
            bail!("parallel report exceeds the declared stride");
        }
        Ok(unit as usize * layout.report_stride)
    }

    pub(crate) fn parallel_read_report(&self, unit: u32, report: &mut [u8]) -> Result<()> {
        let at = self.report_at(unit, report.len())?;
        if let Some(region) = &self.parallel()?.reports {
            // SAFETY: as above.
            report.copy_from_slice(&unsafe { region.slice() }[at..][..report.len()]);
        }
        Ok(())
    }

    pub(crate) fn parallel_write_report(&mut self, unit: u32, report: &[u8]) -> Result<()> {
        let at = self.report_at(unit, report.len())?;
        if let Some(region) = &mut self.parallel.as_mut().expect("checked above").reports {
            // SAFETY: as above.
            let slice = unsafe { region.slice_mut() };
            slice[at..][..report.len()].copy_from_slice(report);
        }
        Ok(())
    }

    pub(crate) fn parallel_write_mix_slot(&mut self, unit: u32, samples: &[f32]) -> Result<()> {
        let layout = self.parallel()?.layout;
        if unit as usize >= layout.max_units {
            bail!("parallel mix unit {unit} is beyond max_units");
        }
        if samples.len() > layout.mix_slot_samples {
            bail!("parallel mix slot cannot hold this block");
        }
        let region = &mut self.parallel.as_mut().expect("checked above").mix;
        // SAFETY: as above.
        let slice = unsafe { region.slice_mut() };
        slice[unit as usize * layout.mix_slot_samples..][..samples.len()].copy_from_slice(samples);
        Ok(())
    }

    /// The serial post-stage on the coordinator, the final block into
    /// `output`.
    pub(crate) fn parallel_end_block(&mut self, output: &mut [f32], frames: u32) -> Result<()> {
        let api = self.parallel()?.api;
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if output.len() != output_samples {
            bail!("audio buffer length does not match prepared output channels");
        }
        let output_channels = self.prepared_output_channels as i32;
        let status =
            self.direct(|_, at| unsafe { (api.end_block)(at, frames as i32, output_channels) });
        check_status(status, "parallel_end_block")?;
        // SAFETY: as above.
        output.copy_from_slice(&unsafe { self.output.slice() }[..output_samples]);
        Ok(())
    }
}

impl Drop for LibraryInstance {
    fn drop(&mut self) {
        // The processor is dropped where it was built: its teardown may be
        // as deep as its construction.
        self.control(|api, at| unsafe { (api.destroy)(at) });
    }
}
