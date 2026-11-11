// RackForge fork of asio-sys 0.2.6 (Apache-2.0). Modified by the RackForge
// project: the driver's own reports of lost audio -- `kAsioOverload`,
// `kAsioResyncRequest` and sample positions that skip a whole buffer -- are
// counted and exposed through `driver_dropouts()` instead of being ignored;
// the driver's requests to be reset are counted and exposed through
// `driver_reset_requests()`; and `open_control_panel()` shows the driver's
// own settings window. See vendor/asio-sys/FORK.md.

pub mod asio_import;
#[macro_use]
pub mod errors;

use self::errors::{AsioError, AsioErrorWrapper, LoadDriverError};
use num_traits::FromPrimitive;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_void};
use std::ptr::null_mut;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex, MutexGuard, Weak,
};

// On Windows (where ASIO actually runs), c_long is i32.
// On non-Windows platforms (for docs.rs and local testing), redefine c_long as i32 to match.
#[cfg(target_os = "windows")]
use std::os::raw::c_long;
#[cfg(not(target_os = "windows"))]
type c_long = i32;

// Bindings import
use self::asio_import as ai;

/// The driver's own account of audio it lost, since the process started.
///
/// A host that times its callbacks sees only its own lateness. A driver can
/// lose a buffer while calling the host punctually -- a USB transfer that
/// missed its slot, a DPC from another driver holding the CPU -- and the only
/// witnesses are the driver's messages and its sample position. Each field is
/// one of them; they overlap, since a driver may report the same loss twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriverDropouts {
    /// `kAsioOverload`: the driver detected an overload. JUCE counts this as
    /// its ASIO xrun.
    pub overloads: u64,
    /// `kAsioResyncRequest`: "the driver encountered some non fatal data
    /// loss". RtAudio reports this as an output underflow.
    pub resyncs: u64,
    /// Buffers the sample position skipped: consecutive buffer switches whose
    /// positions differ by more than one buffer. Catches drivers that lose
    /// audio without sending either message.
    pub skipped_buffers: u64,
}

static DRIVER_OVERLOADS: AtomicU64 = AtomicU64::new(0);
static DRIVER_RESET_REQUESTS: AtomicU64 = AtomicU64::new(0);
static DRIVER_RESYNCS: AtomicU64 = AtomicU64::new(0);
static SKIPPED_BUFFERS: AtomicU64 = AtomicU64::new(0);
/// Frames per buffer of the buffers created last, or 0 when none exist.
static ACTIVE_BUFFER_FRAMES: AtomicI64 = AtomicI64::new(0);
/// Sample position of the previous buffer switch, or -1 when there is none
/// to compare against: before the first switch after `ASIOStart`, and after
/// a switch that reported no valid position.
static PREVIOUS_SAMPLE_POSITION: AtomicI64 = AtomicI64::new(-1);

/// The driver-reported dropouts since the process started.
pub fn driver_dropouts() -> DriverDropouts {
    DriverDropouts {
        overloads: DRIVER_OVERLOADS.load(Ordering::Relaxed),
        resyncs: DRIVER_RESYNCS.load(Ordering::Relaxed),
        skipped_buffers: SKIPPED_BUFFERS.load(Ordering::Relaxed),
    }
}

/// How many times the driver has asked to be reset, since the process
/// started.
///
/// A driver asks when something it owns changed under the host: its buffer
/// size or sample rate was changed in its own control panel, its clock
/// source moved, the device was unplugged. The request cannot be honoured
/// from inside the message -- the driver is on the stack -- so all this does
/// is count, and the host compares the count with the one it saw when it
/// opened its stream. Upstream acknowledges the message and nothing hears
/// it, so the host found out only when the driver stopped calling back.
pub fn driver_reset_requests() -> u64 {
    DRIVER_RESET_REQUESTS.load(Ordering::Relaxed)
}

/// Shows the loaded driver's own settings window.
///
/// Every driver draws its own: ASIO4ALL its device list, a Focusrite its
/// control application, RME its settings dialog. Some are modal and return
/// when closed; others start a separate program and return at once. Either
/// way, a setting changed there reaches the host as a reset request, not as
/// a return value -- see `driver_reset_requests`.
///
/// Call it on the thread that loaded the driver. ASIO drivers are in-process
/// COM objects, and a call from another thread goes to the object without
/// the apartment it was created in; some drivers tolerate that, and the ones
/// that do not fail in ways that are hard to attribute. With no driver loaded
/// the SDK answers `ASE_NotPresent`.
pub fn open_control_panel() -> Result<(), AsioError> {
    asio_result!(unsafe { ai::ASIOControlPanel() })
}

/// How many whole buffers a step in sample position skipped.
///
/// A healthy driver advances by exactly one buffer per switch. ASIO 1.0
/// drivers are asked for their position when the switch arrives, so theirs
/// jitters by a fraction of a buffer; rounding absorbs that. A step that went
/// backwards or not at all is a driver whose position is not usable, not a
/// dropout, and counts nothing.
fn buffers_skipped(step: i64, buffer_frames: i64) -> u64 {
    if buffer_frames <= 0 || step <= 0 {
        return 0;
    }
    let buffers = (step + buffer_frames / 2) / buffer_frames;
    buffers.saturating_sub(1) as u64
}

/// Records one buffer switch's sample position and counts what it skipped.
fn observe_sample_position(position: Option<i64>) {
    let Some(position) = position else {
        PREVIOUS_SAMPLE_POSITION.store(-1, Ordering::Relaxed);
        return;
    };
    let previous = PREVIOUS_SAMPLE_POSITION.swap(position, Ordering::Relaxed);
    if previous < 0 {
        return;
    }
    let skipped = buffers_skipped(
        position - previous,
        ACTIVE_BUFFER_FRAMES.load(Ordering::Relaxed),
    );
    if skipped > 0 {
        SKIPPED_BUFFERS.fetch_add(skipped, Ordering::Relaxed);
    }
}

/// A handle to the ASIO API.
///
/// There should only be one instance of this type at any point in time.
#[derive(Debug, Default)]
pub struct Asio {
    // Keeps track of whether or not a driver is already loaded.
    //
    // This is necessary as ASIO only supports one `Driver` at a time.
    loaded_driver: Mutex<Weak<DriverInner>>,
}

/// A handle to a single ASIO driver.
///
/// Creating an instance of this type loads and initialises the driver.
///
/// Dropping all `Driver` instances will automatically dispose of any resources and de-initialise
/// the driver.
#[derive(Clone, Debug)]
pub struct Driver {
    inner: Arc<DriverInner>,
}

// Contains the state associated with a `Driver`.
//
// This state may be shared between multiple `Driver` handles representing the same underlying
// driver. Only when the last `Driver` is dropped will the `Drop` implementation for this type run
// and the necessary driver resources will be de-allocated and unloaded.
//
// The same could be achieved by returning an `Arc<Driver>` from the `Host::load_driver` API,
// however the `DriverInner` abstraction is required in order to allow for the `Driver::destroy`
// method to exist safely. By wrapping the `Arc<DriverInner>` in the `Driver` type, we can make
// sure the user doesn't `try_unwrap` the `Arc` and invalidate the `Asio` instance's weak pointer.
// This would allow for instantiation of a separate driver before the existing one is destroyed,
// which is disallowed by ASIO.
#[derive(Debug)]
struct DriverInner {
    state: Mutex<DriverState>,
    // The unique name associated with this driver.
    name: String,
    // Track whether or not the driver has been destroyed.
    //
    // This allows for the user to manually destroy the driver and handle any errors if they wish.
    //
    // In the case that the driver has been manually destroyed this flag will be set to `true`
    // indicating to the `drop` implementation that there is nothing to be done.
    destroyed: bool,
}

/// All possible states of an ASIO `Driver` instance.
///
/// Mapped to the finite state machine in the ASIO SDK docs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DriverState {
    Initialized,
    Prepared,
    Running,
}

/// Amount of input and output
/// channels available.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Channels {
    pub ins: c_long,
    pub outs: c_long,
}

/// Sample rate of the ASIO driver.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SampleRate {
    pub rate: u32,
}

/// Information provided to the BufferCallback.
#[derive(Debug)]
pub struct CallbackInfo {
    pub buffer_index: i32,
    pub system_time: ai::ASIOTimeStamp,
    pub callback_flag: u32,
}

/// Holds the pointer to the callbacks that come from cpal
struct BufferCallback(Box<dyn FnMut(&CallbackInfo) + Send>);

/// Input and Output streams.
///
/// There is only ever max one input and one output.
///
/// Only one is required.
pub struct AsioStreams {
    pub input: Option<AsioStream>,
    pub output: Option<AsioStream>,
}

/// A stream to ASIO.
///
/// Contains the buffers.
pub struct AsioStream {
    /// A Double buffer per channel
    pub buffer_infos: Vec<AsioBufferInfo>,
    /// Size of each buffer
    pub buffer_size: i32,
}

/// All the possible types from ASIO.
/// This is a direct copy of the ASIOSampleType
/// inside ASIO SDK.
#[derive(Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioSampleType {
    ASIOSTInt16MSB = 0,
    ASIOSTInt24MSB = 1, // used for 20 bits as well
    ASIOSTInt32MSB = 2,
    ASIOSTFloat32MSB = 3, // IEEE 754 32 bit float
    ASIOSTFloat64MSB = 4, // IEEE 754 64 bit double float

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can be more easily used with these
    ASIOSTInt32MSB16 = 8,  // 32 bit data with 16 bit alignment
    ASIOSTInt32MSB18 = 9,  // 32 bit data with 18 bit alignment
    ASIOSTInt32MSB20 = 10, // 32 bit data with 20 bit alignment
    ASIOSTInt32MSB24 = 11, // 32 bit data with 24 bit alignment

    ASIOSTInt16LSB = 16,
    ASIOSTInt24LSB = 17, // used for 20 bits as well
    ASIOSTInt32LSB = 18,
    ASIOSTFloat32LSB = 19, // IEEE 754 32 bit float, as found on Intel x86 architecture
    ASIOSTFloat64LSB = 20, // IEEE 754 64 bit double float, as found on Intel x86 architecture

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can more easily used with these
    ASIOSTInt32LSB16 = 24, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB18 = 25, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB20 = 26, // 32 bit data with 20 bit alignment
    ASIOSTInt32LSB24 = 27, // 32 bit data with 24 bit alignment

    //	ASIO DSD format.
    ASIOSTDSDInt8LSB1 = 32, // DSD 1 bit data, 8 samples per byte. First sample in Least significant bit.
    ASIOSTDSDInt8MSB1 = 33, // DSD 1 bit data, 8 samples per byte. First sample in Most significant bit.
    ASIOSTDSDInt8NER8 = 40, // DSD 8 bit data, 1 sample per byte. No Endianness required.

    ASIOSTLastEntry,
}

/// Gives information about buffers
/// Receives pointers to buffers
#[derive(Debug, Copy, Clone)]
#[repr(C, packed(4))]
pub struct AsioBufferInfo {
    /// 0 for output 1 for input
    pub is_input: c_long,
    /// Which channel. Starts at 0
    pub channel_num: c_long,
    /// Pointer to each half of the double buffer.
    pub buffers: [*mut c_void; 2],
}

/// Callbacks that ASIO calls
#[repr(C, packed(4))]
struct AsioCallbacks {
    buffer_switch: extern "C" fn(double_buffer_index: c_long, direct_process: c_long) -> (),
    sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
    asio_message: extern "C" fn(
        selector: c_long,
        value: c_long,
        message: *mut (),
        opt: *mut c_double,
    ) -> c_long,
    buffer_switch_time_info: extern "C" fn(
        params: *mut ai::ASIOTime,
        double_buffer_index: c_long,
        direct_process: c_long,
    ) -> *mut ai::ASIOTime,
}

static ASIO_CALLBACKS: AsioCallbacks = AsioCallbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

/// All the possible types from ASIO.
/// This is a direct copy of the asioMessage selectors
/// inside ASIO SDK.
#[rustfmt::skip]
#[derive(Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioMessageSelectors {
    kAsioSelectorSupported = 1, // selector in <value>, returns 1L if supported,
                                // 0 otherwise
    kAsioEngineVersion,         // returns engine (host) asio implementation version,
                                // 2 or higher
    kAsioResetRequest,          // request driver reset. if accepted, this
                                // will close the driver (ASIO_Exit() ) and
                                // re-open it again (ASIO_Init() etc). some
                                // drivers need to reconfigure for instance
                                // when the sample rate changes, or some basic
                                // changes have been made in ASIO_ControlPanel().
                                // returns 1L; note the request is merely passed
                                // to the application, there is no way to determine
                                // if it gets accepted at this time (but it usually
                                // will be).
    kAsioBufferSizeChange,      // not yet supported, will currently always return 0L.
                                // for now, use kAsioResetRequest instead.
                                // once implemented, the new buffer size is expected
                                // in <value>, and on success returns 1L
    kAsioResyncRequest,         // the driver went out of sync, such that
                                // the timestamp is no longer valid. this
                                // is a request to re-start the engine and
                                // slave devices (sequencer). returns 1 for ok,
                                // 0 if not supported.
    kAsioLatenciesChanged,      // the drivers latencies have changed. The engine
                                // will refetch the latencies.
    kAsioSupportsTimeInfo,      // if host returns true here, it will expect the
                                // callback bufferSwitchTimeInfo to be called instead
                                // of bufferSwitch
    kAsioSupportsTimeCode,      //
    kAsioMMCCommand,            // unused - value: number of commands, message points to mmc commands
    kAsioSupportsInputMonitor,  // kAsioSupportsXXX return 1 if host supports this
    kAsioSupportsInputGain,     // unused and undefined
    kAsioSupportsInputMeter,    // unused and undefined
    kAsioSupportsOutputGain,    // unused and undefined
    kAsioSupportsOutputMeter,   // unused and undefined
    kAsioOverload,              // driver detected an overload

    kAsioNumMessageSelectors
}

/// A rust-usable version of the `ASIOTime` type that does not contain a binary blob for fields.
#[repr(C, packed(4))]
pub struct AsioTime {
    /// Must be `0`.
    pub reserved: [c_long; 4],
    /// Required.
    pub time_info: AsioTimeInfo,
    /// Optional, evaluated if (time_code.flags & ktcValid).
    pub time_code: AsioTimeCode,
}

/// A rust-compatible version of the `ASIOTimeInfo` type that does not contain a binary blob for
/// fields.
#[repr(C, packed(4))]
pub struct AsioTimeInfo {
    /// Absolute speed (1. = nominal).
    pub speed: c_double,
    /// System time related to sample_position, in nanoseconds.
    ///
    /// On Windows, must be derived from timeGetTime().
    pub system_time: ai::ASIOTimeStamp,
    /// Sample position since `ASIOStart()`.
    pub sample_position: ai::ASIOSamples,
    /// Current rate, unsigned.
    pub sample_rate: AsioSampleRate,
    /// See `AsioTimeInfoFlags`.
    pub flags: c_long,
    /// Must be `0`.
    pub reserved: [c_char; 12],
}

/// A rust-compatible version of the `ASIOTimeCode` type that does not use a binary blob for its
/// fields.
#[repr(C, packed(4))]
pub struct AsioTimeCode {
    /// Speed relation (fraction of nominal speed) optional.
    ///
    /// Set to 0. or 1. if not supported.
    pub speed: c_double,
    /// Time in samples unsigned.
    pub time_code_samples: ai::ASIOSamples,
    /// See `ASIOTimeCodeFlags`.
    pub flags: c_long,
    /// Set to `0`.
    pub future: [c_char; 64],
}

/// A rust-compatible version of the `ASIOSampleRate` type that does not use a binary blob for its
/// fields.
pub type AsioSampleRate = f64;

// A helper type to simplify retrieval of available buffer sizes.
#[derive(Default)]
struct BufferSizes {
    min: c_long,
    max: c_long,
    pref: c_long,
    grans: c_long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackId(usize);

/// A global way to access all the callbacks.
///
/// This is required because of how ASIO calls the `buffer_switch` function with no data
/// parameters.
static BUFFER_CALLBACK: Mutex<Vec<(CallbackId, BufferCallback)>> = Mutex::new(Vec::new());

/// Used to identify when to clear buffers.
static CALLBACK_FLAG: AtomicU32 = AtomicU32::new(0);

/// Indicates that ASIOOutputReady should be called
static CALL_OUTPUT_READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageCallbackId(usize);

struct MessageCallback(Arc<dyn Fn(AsioMessageSelectors) + Send + Sync>);

/// A global registry for ASIO message callbacks.
static MESSAGE_CALLBACKS: Mutex<Vec<(MessageCallbackId, MessageCallback)>> = Mutex::new(Vec::new());

impl Asio {
    /// Initialise the ASIO API.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the name for each available driver.
    ///
    /// This is used at the start to allow the user to choose which driver they want.
    pub fn driver_names(&self) -> Vec<String> {
        // The most drivers we can take
        const MAX_DRIVERS: usize = 100;
        // Max length for divers name
        const MAX_DRIVER_NAME_LEN: usize = 32;

        // 2D array of driver names set to 0.
        let mut driver_names: [[c_char; MAX_DRIVER_NAME_LEN]; MAX_DRIVERS] =
            [[0; MAX_DRIVER_NAME_LEN]; MAX_DRIVERS];
        // Pointer to each driver name.
        let mut driver_name_ptrs: [*mut i8; MAX_DRIVERS] = [null_mut(); MAX_DRIVERS];
        for (ptr, name) in driver_name_ptrs.iter_mut().zip(&mut driver_names[..]) {
            *ptr = (*name).as_mut_ptr();
        }

        unsafe {
            let num_drivers =
                ai::get_driver_names(driver_name_ptrs.as_mut_ptr(), MAX_DRIVERS as i32);
            (0..num_drivers)
                .map(|i| driver_name_to_utf8(&driver_names[i as usize]).to_string())
                .collect()
        }
    }

    /// If a driver has already been loaded, this will return that driver.
    ///
    /// Returns `None` if no driver is currently loaded.
    ///
    /// This can be useful to check before calling `load_driver` as ASIO only supports loading a
    /// single driver at a time.
    pub fn loaded_driver(&self) -> Option<Driver> {
        self.loaded_driver
            .lock()
            .expect("failed to acquire loaded driver lock")
            .upgrade()
            .map(|inner| Driver { inner })
    }

    /// Load a driver from the given name.
    ///
    /// Driver names compatible with this method can be produced via the `asio.driver_names()`
    /// method.
    ///
    /// NOTE: Despite many requests from users, ASIO only supports loading a single driver at a
    /// time. Calling this method while a previously loaded `Driver` instance exists will result in
    /// an error. That said, if this method is called with the name of a driver that has already
    /// been loaded, that driver will be returned successfully.
    pub fn load_driver(&self, driver_name: &str) -> Result<Driver, LoadDriverError> {
        // Check whether or not a driver is already loaded.
        if let Some(driver) = self.loaded_driver() {
            if driver.name() == driver_name {
                return Ok(driver);
            } else {
                return Err(LoadDriverError::DriverAlreadyExists);
            }
        }

        // Make owned CString to send to load driver
        let driver_name_cstring =
            CString::new(driver_name).expect("failed to create `CString` from driver name");
        let mut driver_info = std::mem::MaybeUninit::<ai::ASIODriverInfo>::uninit();

        unsafe {
            // TODO: Check that a driver of the same name does not already exist?
            match ai::load_asio_driver(driver_name_cstring.as_ptr() as *mut i8) {
                false => Err(LoadDriverError::LoadDriverFailed),
                true => {
                    // Initialize ASIO.
                    asio_result!(ai::ASIOInit(driver_info.as_mut_ptr()))?;
                    let _driver_info = driver_info.assume_init();
                    let state = Mutex::new(DriverState::Initialized);
                    let name = driver_name.to_string();
                    let destroyed = false;
                    let inner = Arc::new(DriverInner {
                        name,
                        state,
                        destroyed,
                    });
                    *self
                        .loaded_driver
                        .lock()
                        .expect("failed to acquire loaded driver lock") = Arc::downgrade(&inner);
                    let driver = Driver { inner };
                    Ok(driver)
                }
            }
        }
    }
}

impl BufferCallback {
    /// Calls the inner callback.
    fn run(&mut self, callback_info: &CallbackInfo) {
        let cb = &mut self.0;
        cb(callback_info);
    }
}

impl Driver {
    /// The name used to uniquely identify this driver.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the number of input and output channels available on the driver.
    pub fn channels(&self) -> Result<Channels, AsioError> {
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            asio_result!(ai::ASIOGetChannels(&mut ins, &mut outs))?;
        }
        let channel = Channels { ins, outs };
        Ok(channel)
    }

    /// Get the min and max supported buffersize of the driver.
    pub fn buffersize_range(&self) -> Result<(c_long, c_long), AsioError> {
        let buffer_sizes = asio_get_buffer_sizes()?;
        let min = buffer_sizes.min;
        let max = buffer_sizes.max;
        Ok((min, max))
    }

    /// Get current sample rate of the driver.
    pub fn sample_rate(&self) -> Result<c_double, AsioError> {
        let mut rate: c_double = 0.0;
        unsafe {
            asio_result!(ai::get_sample_rate(&mut rate))?;
        }
        Ok(rate)
    }

    /// Can the driver accept the given sample rate.
    pub fn can_sample_rate(&self, sample_rate: c_double) -> Result<bool, AsioError> {
        unsafe {
            match asio_result!(ai::can_sample_rate(sample_rate)) {
                Ok(()) => Ok(true),
                Err(AsioError::NoRate) => Ok(false),
                Err(err) => Err(err),
            }
        }
    }

    /// Set the sample rate for the driver.
    pub fn set_sample_rate(&self, sample_rate: c_double) -> Result<(), AsioError> {
        unsafe {
            asio_result!(ai::set_sample_rate(sample_rate))?;
        }
        Ok(())
    }

    /// Get the current data type of the driver's input stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn input_data_type(&self) -> Result<AsioSampleType, AsioError> {
        stream_data_type(true)
    }

    /// Get the current data type of the driver's output stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn output_data_type(&self) -> Result<AsioSampleType, AsioError> {
        stream_data_type(false)
    }

    /// Ask ASIO to allocate the buffers and give the callback pointers.
    ///
    /// This will destroy any already allocated buffers.
    ///
    /// If buffersize is None then the preferred buffer size from ASIO is used,
    /// otherwise the desired buffersize is used if the requested size is within
    /// the range of accepted buffersizes for the device.
    fn create_buffers(
        &self,
        buffer_infos: &mut [AsioBufferInfo],
        buffer_size: Option<i32>,
    ) -> Result<c_long, AsioError> {
        let num_channels = buffer_infos.len();

        let mut state = self.inner.lock_state();

        // Retrieve the available buffer sizes.
        let buffer_sizes = asio_get_buffer_sizes()?;
        if buffer_sizes.pref <= 0 {
            panic!(
                "`ASIOGetBufferSize` produced unusable preferred buffer size of {}",
                buffer_sizes.pref,
            );
        }

        let buffer_size = match buffer_size {
            Some(v) => {
                if v <= buffer_sizes.max {
                    v
                } else {
                    return Err(AsioError::InvalidBufferSize);
                }
            }
            None => buffer_sizes.pref,
        };

        CALL_OUTPUT_READY.store(
            asio_result!(unsafe { ai::ASIOOutputReady() }).is_ok(),
            Ordering::Release,
        );

        // Ensure the driver is in the `Initialized` state.
        if let DriverState::Running = *state {
            state.stop()?;
        }
        if let DriverState::Prepared = *state {
            state.dispose_buffers()?;
        }
        unsafe {
            asio_result!(ai::ASIOCreateBuffers(
                buffer_infos.as_mut_ptr() as *mut _,
                num_channels as i32,
                buffer_size,
                &ASIO_CALLBACKS as *const _ as *mut _,
            ))?;
        }
        *state = DriverState::Prepared;
        ACTIVE_BUFFER_FRAMES.store(i64::from(buffer_size), Ordering::Relaxed);
        PREVIOUS_SAMPLE_POSITION.store(-1, Ordering::Relaxed);

        Ok(buffer_size)
    }

    /// Creates the streams.
    ///
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// Both input and output streams need to be created together as a single slice of
    /// `ASIOBufferInfo`.
    fn create_streams(
        &self,
        mut input_buffer_infos: Vec<AsioBufferInfo>,
        mut output_buffer_infos: Vec<AsioBufferInfo>,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let (input, output) = match (
            input_buffer_infos.is_empty(),
            output_buffer_infos.is_empty(),
        ) {
            // Both stream exist.
            (false, false) => {
                // Create one continuous slice of buffers.
                let split_point = input_buffer_infos.len();
                let mut all_buffer_infos = input_buffer_infos;
                all_buffer_infos.append(&mut output_buffer_infos);
                // Create the buffers. On success, split the output and input again.
                let buffer_size = self.create_buffers(&mut all_buffer_infos, buffer_size)?;
                let output_buffer_infos = all_buffer_infos.split_off(split_point);
                let input_buffer_infos = all_buffer_infos;
                let input = Some(AsioStream {
                    buffer_infos: input_buffer_infos,
                    buffer_size,
                });
                let output = Some(AsioStream {
                    buffer_infos: output_buffer_infos,
                    buffer_size,
                });
                (input, output)
            }
            // Just input
            (false, true) => {
                let buffer_size = self.create_buffers(&mut input_buffer_infos, buffer_size)?;
                let input = Some(AsioStream {
                    buffer_infos: input_buffer_infos,
                    buffer_size,
                });
                let output = None;
                (input, output)
            }
            // Just output
            (true, false) => {
                let buffer_size = self.create_buffers(&mut output_buffer_infos, buffer_size)?;
                let input = None;
                let output = Some(AsioStream {
                    buffer_infos: output_buffer_infos,
                    buffer_size,
                });
                (input, output)
            }
            // Impossible
            (true, true) => unreachable!("Trying to create streams without preparing"),
        };
        Ok(AsioStreams { input, output })
    }

    /// Prepare the input stream.
    ///
    /// Because only the latest call to ASIOCreateBuffers is relevant this call will destroy all
    /// past active buffers and recreate them.
    ///
    /// For this reason we take the output stream if it exists.
    ///
    /// `num_channels` is the desired number of input channels.
    ///
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// This returns a full AsioStreams with both input and output if output was active.
    pub fn prepare_input_stream(
        &self,
        output: Option<AsioStream>,
        num_channels: usize,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = prepare_buffer_infos(true, num_channels);
        let output_buffer_infos = output.map(|output| output.buffer_infos).unwrap_or_default();
        self.create_streams(input_buffer_infos, output_buffer_infos, buffer_size)
    }

    /// Prepare the output stream.
    ///
    /// Because only the latest call to ASIOCreateBuffers is relevant this call will destroy all
    /// past active buffers and recreate them.
    ///
    /// For this reason we take the input stream if it exists.
    ///
    /// `num_channels` is the desired number of output channels.
    ///
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// This returns a full AsioStreams with both input and output if input was active.
    pub fn prepare_output_stream(
        &self,
        input: Option<AsioStream>,
        num_channels: usize,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = input.map(|input| input.buffer_infos).unwrap_or_default();
        let output_buffer_infos = prepare_buffer_infos(false, num_channels);
        self.create_streams(input_buffer_infos, output_buffer_infos, buffer_size)
    }

    /// Releases buffers allocations.
    ///
    /// This will `stop` the stream if the driver is `Running`.
    ///
    /// No-op if no buffers are allocated.
    pub fn dispose_buffers(&self) -> Result<(), AsioError> {
        self.inner.dispose_buffers_inner()
    }

    /// Starts ASIO streams playing.
    ///
    /// The driver must be in the `Prepared` state
    ///
    /// If called successfully, the driver will be in the `Running` state.
    ///
    /// No-op if already `Running`.
    pub fn start(&self) -> Result<(), AsioError> {
        let mut state = self.inner.lock_state();
        if let DriverState::Running = *state {
            return Ok(());
        }
        // Positions count from `ASIOStart`, so the last one before a restart
        // is not comparable with the first one after it.
        PREVIOUS_SAMPLE_POSITION.store(-1, Ordering::Relaxed);
        unsafe {
            asio_result!(ai::ASIOStart())?;
        }
        *state = DriverState::Running;
        Ok(())
    }

    /// Stops ASIO streams playing.
    ///
    /// No-op if the state is not `Running`.
    ///
    /// If the state was `Running` and the stream is stopped successfully, the driver will be in
    /// the `Prepared` state.
    pub fn stop(&self) -> Result<(), AsioError> {
        self.inner.stop_inner()
    }

    /// Adds a callback to the list of active callbacks.
    ///
    /// The given function receives the index of the buffer currently ready for processing.
    ///
    /// Returns an ID uniquely associated with the given callback so that it may be removed later.
    pub fn add_callback<F>(&self, callback: F) -> CallbackId
    where
        F: 'static + FnMut(&CallbackInfo) + Send,
    {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        let id = bc
            .last()
            .map(|&(id, _)| CallbackId(id.0.checked_add(1).expect("stream ID overflowed")))
            .unwrap_or(CallbackId(0));
        let cb = BufferCallback(Box::new(callback));
        bc.push((id, cb));
        id
    }

    /// Remove the callback with the given ID.
    pub fn remove_callback(&self, rem_id: CallbackId) {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        bc.retain(|&(id, _)| id != rem_id);
    }

    /// Consumes and destroys the `Driver`, stopping the streams if they are running and releasing
    /// any associated resources.
    ///
    /// Returns `Ok(true)` if the driver was successfully destroyed.
    ///
    /// Returns `Ok(false)` if the driver was not destroyed because another handle to the driver
    /// still exists.
    ///
    /// Returns `Err` if some switching driver states failed or if ASIO returned an error on exit.
    pub fn destroy(self) -> Result<bool, AsioError> {
        let Driver { inner } = self;
        match Arc::try_unwrap(inner) {
            Err(_) => Ok(false),
            Ok(mut inner) => {
                inner.destroy_inner()?;
                Ok(true)
            }
        }
    }

    /// Adds a callback to the list of message listeners.
    ///
    /// Returns an ID uniquely associated with the given callback so that it may be removed later.
    pub fn add_message_callback<F>(&self, callback: F) -> MessageCallbackId
    where
        F: Fn(AsioMessageSelectors) + Send + Sync + 'static,
    {
        let mut mcb = MESSAGE_CALLBACKS.lock().unwrap();
        let id = mcb
            .last()
            .map(|&(id, _)| {
                MessageCallbackId(id.0.checked_add(1).expect("MessageCallbackId overflowed"))
            })
            .unwrap_or(MessageCallbackId(0));

        let cb = MessageCallback(Arc::new(callback));
        mcb.push((id, cb));
        id
    }

    /// Remove the callback with the given ID.
    pub fn remove_message_callback(&self, rem_id: MessageCallbackId) {
        let mut mcb = MESSAGE_CALLBACKS.lock().unwrap();
        mcb.retain(|&(id, _)| id != rem_id);
    }
}

impl DriverState {
    fn stop(&mut self) -> Result<(), AsioError> {
        if let DriverState::Running = *self {
            unsafe {
                asio_result!(ai::ASIOStop())?;
            }
            *self = DriverState::Prepared;
        }
        Ok(())
    }

    fn dispose_buffers(&mut self) -> Result<(), AsioError> {
        if let DriverState::Initialized = *self {
            return Ok(());
        }
        if let DriverState::Running = *self {
            self.stop()?;
        }
        unsafe {
            asio_result!(ai::ASIODisposeBuffers())?;
        }
        *self = DriverState::Initialized;
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), AsioError> {
        if let DriverState::Running = *self {
            self.stop()?;
        }
        if let DriverState::Prepared = *self {
            self.dispose_buffers()?;
        }
        unsafe {
            asio_result!(ai::ASIOExit())?;
            ai::remove_current_driver();
        }
        Ok(())
    }
}

impl DriverInner {
    fn lock_state(&self) -> MutexGuard<'_, DriverState> {
        self.state.lock().expect("failed to lock `DriverState`")
    }

    fn stop_inner(&self) -> Result<(), AsioError> {
        let mut state = self.lock_state();
        state.stop()
    }

    fn dispose_buffers_inner(&self) -> Result<(), AsioError> {
        let mut state = self.lock_state();
        state.dispose_buffers()
    }

    fn destroy_inner(&mut self) -> Result<(), AsioError> {
        {
            let mut state = self.lock_state();
            state.destroy()?;

            // Clear any existing stream callbacks.
            if let Ok(mut bcs) = BUFFER_CALLBACK.lock() {
                bcs.clear();
            }
        }

        // Signal that the driver has been destroyed.
        self.destroyed = true;

        Ok(())
    }
}

impl Drop for DriverInner {
    fn drop(&mut self) {
        if !self.destroyed {
            // We probably shouldn't `panic!` in the destructor? We also shouldn't ignore errors
            // though either.
            self.destroy_inner().ok();
        }
    }
}

unsafe impl Send for AsioStream {}

/// Used by the input and output stream creation process.
fn prepare_buffer_infos(is_input: bool, n_channels: usize) -> Vec<AsioBufferInfo> {
    let is_input = if is_input { 1 } else { 0 };
    (0..n_channels)
        .map(|ch| {
            let channel_num = ch as c_long;
            // To be filled by ASIOCreateBuffers.
            let buffers = [std::ptr::null_mut(); 2];
            AsioBufferInfo {
                is_input,
                channel_num,
                buffers,
            }
        })
        .collect()
}

/// Retrieve the minimum, maximum and preferred buffer sizes along with the available
/// buffer size granularity.
fn asio_get_buffer_sizes() -> Result<BufferSizes, AsioError> {
    let mut b = BufferSizes::default();
    unsafe {
        let res = ai::ASIOGetBufferSize(&mut b.min, &mut b.max, &mut b.pref, &mut b.grans);
        asio_result!(res)?;
    }
    Ok(b)
}

/// Retrieve the `ASIOChannelInfo` associated with the channel at the given index on either the
/// input or output stream (`true` for input).
fn asio_channel_info(channel: c_long, is_input: bool) -> Result<ai::ASIOChannelInfo, AsioError> {
    let mut channel_info = ai::ASIOChannelInfo {
        // Which channel we are querying
        channel,
        // Was it input or output
        isInput: if is_input { 1 } else { 0 },
        // Was it active
        isActive: 0,
        channelGroup: 0,
        // The sample type
        type_: 0,
        name: [0 as c_char; 32],
    };
    unsafe {
        asio_result!(ai::ASIOGetChannelInfo(&mut channel_info))?;
        Ok(channel_info)
    }
}

/// Retrieve the data type of either the input or output stream.
///
/// If `is_input` is true, this will be queried on the input stream.
fn stream_data_type(is_input: bool) -> Result<AsioSampleType, AsioError> {
    let channel_info = asio_channel_info(0, is_input)?;
    Ok(FromPrimitive::from_i32(channel_info.type_).expect("unknown `ASIOSampletype` value"))
}

/// ASIO uses null terminated c strings for driver names.
///
/// This converts to utf8.
fn driver_name_to_utf8(bytes: &[c_char]) -> std::borrow::Cow<'_, str> {
    unsafe { CStr::from_ptr(bytes.as_ptr()).to_string_lossy() }
}

/// ASIO uses null terminated c strings for channel names.
///
/// This converts to utf8.
fn _channel_name_to_utf8(bytes: &[c_char]) -> std::borrow::Cow<'_, str> {
    unsafe { CStr::from_ptr(bytes.as_ptr()).to_string_lossy() }
}

/// Indicates the stream sample rate has changed.
///
/// TODO: Provide some way of allowing CPAL to handle this.
extern "C" fn sample_rate_did_change(s_rate: c_double) {
    eprintln!("unhandled sample rate change to {}", s_rate);
}

/// Message callback for ASIO to notify of certain events.
extern "C" fn asio_message(
    selector: c_long,
    value: c_long,
    _message: *mut (),
    _opt: *mut c_double,
) -> c_long {
    match AsioMessageSelectors::from_i64(selector as i64) {
        Some(AsioMessageSelectors::kAsioSelectorSupported) => {
            // Indicate what message selectors are supported.
            match AsioMessageSelectors::from_i64(value as i64) {
                | Some(AsioMessageSelectors::kAsioResetRequest)
                | Some(AsioMessageSelectors::kAsioEngineVersion)
                | Some(AsioMessageSelectors::kAsioResyncRequest)
                | Some(AsioMessageSelectors::kAsioLatenciesChanged)
                // Following added in ASIO 2.0.
                | Some(AsioMessageSelectors::kAsioSupportsTimeInfo)
                | Some(AsioMessageSelectors::kAsioSupportsTimeCode)
                | Some(AsioMessageSelectors::kAsioSupportsInputMonitor)
                // Said yes to so the driver knows someone is listening: a
                // driver only sends what the host declared it handles.
                | Some(AsioMessageSelectors::kAsioOverload)
                // Handled the way JUCE handles it: as a reset request.
                | Some(AsioMessageSelectors::kAsioBufferSizeChange) => 1,
                _ => 0,
            }
        }

        Some(AsioMessageSelectors::kAsioResetRequest) => {
            // RackForge fork: counted, so a host that registered no callback
            // -- cpal registers none -- can still find out.
            DRIVER_RESET_REQUESTS.fetch_add(1, Ordering::Relaxed);
            // Defer the task and perform the reset of the driver during the next "safe" situation
            // You cannot reset the driver right now, as this code is called from the driver. Reset
            // the driver is done by completely destruct it. I.e. ASIOStop(), ASIODisposeBuffers(),
            // Destruction. Afterwards you initialize the driver again.

            // Get the list of active message callbacks.
            let callbacks: Vec<_> = {
                let lock = MESSAGE_CALLBACKS.lock().unwrap();
                lock.iter().map(|(_, cb)| cb.0.clone()).collect()
            };
            // Release lock and call them.
            for cb in callbacks {
                cb(AsioMessageSelectors::kAsioResetRequest);
            }

            1
        }

        Some(AsioMessageSelectors::kAsioResyncRequest) => {
            // This informs the application, that the driver encountered some non fatal data loss.
            // It is used for synchronization purposes of different media. Added mainly to work
            // around the Win16Mutex problems in Windows 95/98 with the Windows Multimedia system,
            // which could loose data because the Mutex was hold too long by another thread.
            // However a driver can issue it in other situations, too.
            //
            // Data loss is what a dropout is. Counted, as RtAudio does; the
            // stream is left running, because the loss is already over.
            DRIVER_RESYNCS.fetch_add(1, Ordering::Relaxed);
            1
        }

        Some(AsioMessageSelectors::kAsioBufferSizeChange) => {
            // The buffer size changed, usually in the driver's own panel.
            // The host can only follow it by reopening, which is exactly
            // what a reset request asks for.
            DRIVER_RESET_REQUESTS.fetch_add(1, Ordering::Relaxed);
            1
        }

        Some(AsioMessageSelectors::kAsioOverload) => {
            // The driver detected an overload: it did not get a buffer in
            // time. JUCE counts exactly this as its ASIO xrun.
            DRIVER_OVERLOADS.fetch_add(1, Ordering::Relaxed);
            1
        }

        Some(AsioMessageSelectors::kAsioLatenciesChanged) => {
            // This will inform the host application that the drivers were latencies changed.
            // Beware, it this does not mean that the buffer sizes have changed! You might need to
            // update internal delay data.
            // TODO: Handle this.
            1
        }

        Some(AsioMessageSelectors::kAsioEngineVersion) => {
            // Return the supported ASIO version of the host application If a host applications
            // does not implement this selector, ASIO 1.0 is assumed by the driver
            2
        }

        Some(AsioMessageSelectors::kAsioSupportsTimeInfo) => {
            // Informs the driver whether the asioCallbacks.bufferSwitchTimeInfo() callback is
            // supported. For compatibility with ASIO 1.0 drivers the host application should
            // always support the "old" bufferSwitch method, too, which we do.
            1
        }

        Some(AsioMessageSelectors::kAsioSupportsTimeCode) => {
            // Informs the driver whether the application is interested in time code info. If an
            // application does not need to know about time code, the driver has less work to do.
            // TODO: Provide an option for this?
            1
        }

        _ => 0, // Unknown/unhandled message type.
    }
}

/// Similar to buffer switch but with time info.
///
/// If only `buffer_switch` is called by the driver instead, the `buffer_switch` callback will
/// create the necessary timing info and call this function.
///
/// TODO: Provide some access to `ai::ASIOTime` once CPAL gains support for time stamps.
extern "C" fn buffer_switch_time_info(
    time: *mut ai::ASIOTime,
    double_buffer_index: c_long,
    _direct_process: c_long,
) -> *mut ai::ASIOTime {
    // This lock is probably unavoidable, but locks in the audio stream are not great.
    let mut bcs = BUFFER_CALLBACK.lock().unwrap();
    let asio_time: &mut AsioTime = unsafe { &mut *(time as *mut AsioTime) };
    // Alternates: 0, 1, 0, 1, ...
    let callback_flag = CALLBACK_FLAG.fetch_xor(1, Ordering::Relaxed);

    let flags = asio_time.time_info.flags;
    let position_valid =
        flags & ai::AsioTimeInfoFlags::kSamplePositionValid.0 as c_long != 0;
    observe_sample_position(position_valid.then(|| {
        let samples = asio_time.time_info.sample_position;
        (i64::from(samples.hi) << 32) | i64::from(samples.lo)
    }));

    let callback_info = CallbackInfo {
        buffer_index: double_buffer_index,
        system_time: asio_time.time_info.system_time,
        callback_flag,
    };
    for &mut (_, ref mut bc) in bcs.iter_mut() {
        bc.run(&callback_info);
    }

    if CALL_OUTPUT_READY.load(Ordering::Acquire) {
        unsafe { ai::ASIOOutputReady() };
    }

    time
}

/// This is called by ASIO.
///
/// Here we run the callback for each stream.
///
/// `double_buffer_index` is either `0` or `1`  indicating which buffer to fill.
extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: c_long) {
    // Emulate the time info provided by the `buffer_switch_time_info` callback.
    // This is an attempt at matching the behaviour in `hostsample.cpp` from the SDK.
    let mut time = unsafe {
        let mut time: AsioTime = std::mem::zeroed();
        let res = ai::ASIOGetSamplePosition(
            &mut time.time_info.sample_position,
            &mut time.time_info.system_time,
        );
        if let Ok(()) = asio_result!(res) {
            time.time_info.flags = (ai::AsioTimeInfoFlags::kSystemTimeValid
                | ai::AsioTimeInfoFlags::kSamplePositionValid)
                // Context about the cast:
                //
                // Cast was required to successfully compile with MinGW-w64.
                //
                // The flags defined will not create a value that exceeds the maximum value of an i32.
                // The flags are intended to be non-negative, so the sign bit will not be used.
                // The c_uint (flags) is being cast to i32 which is safe as long as the actual value fits within the i32 range, which is true in this case.
                //
                // The actual flags in asio sdk are defined as:
                // typedef enum AsioTimeInfoFlags
                // {
                //	kSystemTimeValid        = 1,            // must always be valid
                //	kSamplePositionValid    = 1 << 1,       // must always be valid
                //	kSampleRateValid        = 1 << 2,
                //	kSpeedValid             = 1 << 3,
                //
                //	kSampleRateChanged      = 1 << 4,
                //	kClockSourceChanged     = 1 << 5
                // } AsioTimeInfoFlags;
                .0 as _;
        }
        time
    };

    // Actual processing happens within the `buffer_switch_time_info` callback.
    let asio_time_ptr = &mut time as *mut AsioTime as *mut ai::ASIOTime;
    buffer_switch_time_info(asio_time_ptr, double_buffer_index, direct_process);
}

#[test]
fn check_type_sizes() {
    assert_eq!(
        std::mem::size_of::<AsioSampleRate>(),
        std::mem::size_of::<ai::ASIOSampleRate>()
    );
    assert_eq!(
        std::mem::size_of::<AsioTimeCode>(),
        std::mem::size_of::<ai::ASIOTimeCode>()
    );
    assert_eq!(
        std::mem::size_of::<AsioTimeInfo>(),
        std::mem::size_of::<ai::AsioTimeInfo>(),
    );
    assert_eq!(
        std::mem::size_of::<AsioTime>(),
        std::mem::size_of::<ai::ASIOTime>()
    );
}

// RackForge fork: tests for the dropout counting added above.
#[cfg(test)]
mod dropout_tests {
    use super::*;

    #[test]
    fn one_buffer_per_switch_skips_nothing_and_each_extra_buffer_counts_once() {
        assert_eq!(buffers_skipped(128, 128), 0);
        assert_eq!(buffers_skipped(256, 128), 1);
        assert_eq!(buffers_skipped(512, 128), 3);
    }

    #[test]
    fn an_asio_1_position_that_jitters_by_less_than_half_a_buffer_skips_nothing() {
        assert_eq!(buffers_skipped(128 + 60, 128), 0);
        assert_eq!(buffers_skipped(128 - 60, 128), 0);
    }

    #[test]
    fn a_position_that_stalls_or_runs_backwards_is_unusable_not_a_dropout() {
        assert_eq!(buffers_skipped(0, 128), 0);
        assert_eq!(buffers_skipped(-128, 128), 0);
        assert_eq!(buffers_skipped(1_000_000, 0), 0);
    }

    fn send(selector: AsioMessageSelectors, value: c_long) -> c_long {
        asio_message(selector as c_long, value, null_mut(), null_mut())
    }

    #[test]
    fn the_host_declares_every_message_it_now_handles() {
        // A driver only sends what the host said yes to.
        for handled in [
            AsioMessageSelectors::kAsioOverload,
            AsioMessageSelectors::kAsioBufferSizeChange,
            AsioMessageSelectors::kAsioResetRequest,
            AsioMessageSelectors::kAsioResyncRequest,
        ] {
            let name = format!("{handled:?}");
            assert_eq!(
                send(AsioMessageSelectors::kAsioSelectorSupported, handled as c_long),
                1,
                "{name} must be declared supported",
            );
        }
    }

    // Each counter is touched by exactly one test, so the tests can run in
    // parallel without one reading another's increments.
    #[test]
    fn a_reset_request_and_a_buffer_size_change_are_both_counted_as_resets() {
        let before = driver_reset_requests();
        assert_eq!(send(AsioMessageSelectors::kAsioResetRequest, 0), 1);
        assert_eq!(send(AsioMessageSelectors::kAsioBufferSizeChange, 256), 1);
        assert_eq!(driver_reset_requests(), before + 2);
    }

    #[test]
    fn an_overload_and_a_resync_are_counted_as_dropouts() {
        let before = driver_dropouts();
        assert_eq!(send(AsioMessageSelectors::kAsioOverload, 0), 1);
        assert_eq!(send(AsioMessageSelectors::kAsioResyncRequest, 0), 1);
        let after = driver_dropouts();
        assert_eq!(after.overloads, before.overloads + 1);
        assert_eq!(after.resyncs, before.resyncs + 1);
    }

    // The counters are process statics, so the whole sequence lives in one
    // test rather than racing others that touch the same state.
    #[test]
    fn switches_are_compared_only_with_a_valid_position_from_the_same_run() {
        ACTIVE_BUFFER_FRAMES.store(128, Ordering::Relaxed);
        PREVIOUS_SAMPLE_POSITION.store(-1, Ordering::Relaxed);
        let before = driver_dropouts().skipped_buffers;

        observe_sample_position(Some(0));
        observe_sample_position(Some(128));
        assert_eq!(driver_dropouts().skipped_buffers, before);

        // Two buffers went by that the host never saw.
        observe_sample_position(Some(512));
        assert_eq!(driver_dropouts().skipped_buffers, before + 2);

        // A switch without a valid position breaks the chain: the next
        // valid one has nothing to be compared with, however far it is.
        observe_sample_position(None);
        observe_sample_position(Some(1_000_000));
        observe_sample_position(Some(1_000_128));
        assert_eq!(driver_dropouts().skipped_buffers, before + 2);
    }
}
