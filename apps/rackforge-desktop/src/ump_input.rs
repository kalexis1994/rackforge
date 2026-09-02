//! Windows MIDI Services as the desktop's packet transport.
//!
//! Every MIDI input the desktop has had so far came through `midir`, which
//! is WinMM: bytes, and a driver stack that rounds anything wider to them.
//! Windows MIDI Services is the replacement Windows 11 ships -- a service
//! (`midisrv`) that owns every device and speaks Universal MIDI Packets to
//! clients -- and its App SDK is the only public way to receive packets
//! rather than bytes. This module is that client: it hands each packet a
//! connection receives to [`rackforge_core::ump::read_stream`], which turns
//! it into the host's own packets with their width, and everything past that
//! point is the path the byte transports already take.
//!
//! The SDK runtime is not part of Windows and not something an application
//! may redistribute; it is installed separately, and this module says so
//! once and stands aside when it is missing -- the desktop then has exactly
//! the inputs it had before. Bootstrapping goes through the SDK's COM
//! initializer, which loads the runtime and redirects WinRT activation of
//! its classes to it; from then on the generated bindings in `midi2_sdk` are
//! ordinary WinRT calls.
//!
//! A UMP endpoint carries sixteen groups, and the service builds an
//! endpoint out of every MIDI 1.0 port a device has -- the KeyLab's
//! keyboard, its DAW port, its DIN thru -- one group each. Those ports are
//! what the desktop's controller drivers recognise by name, so each group
//! that the service associates with a MIDI 1.0 input port becomes its own
//! source, named `UMP: <that port's name>`: a saved selection tells this
//! transport from the byte ones by the prefix, and the driver that matched
//! `KL Essential 61 mk3 MIDI` over `midir` matches it here. An endpoint
//! without associated ports (a native MIDI 2.0 device) is one source for
//! all its groups. Each message carries the service's timestamp -- the
//! performance counter, the same clock the audio thread can read -- so the
//! audio thread places it at its sample inside the block instead of at the
//! block's first sample.

// The initializer's methods carry the names of the SDK's vtable.
#![allow(non_snake_case, clippy::too_many_arguments)]

use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use windows::Foundation::TypedEventHandler;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows_core::{GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Ref, interface};

use crate::midi2_sdk::Microsoft::Windows::Devices::Midi2::{
    IMidiMessageReceivedEventSource, Midi1PortFlow, MidiClock, MidiEndpointConnection,
    MidiEndpointDeviceInformation, MidiEndpointDevicePurpose, MidiMessageReceivedEventArgs,
    MidiSession,
};

/// What a Windows MIDI Services endpoint is called as a RackForge source.
pub const NAME_PREFIX: &str = "UMP: ";

/// The SDK's desktop-app initializer: a plain COM class the runtime
/// installer registers. Creating it loads the runtime and sets up the
/// activation redirection -- there is no separate `Initialize` in the
/// shipped vtable, whatever the IDL in the repository says; this follows
/// the redistributable header (`Microsoft.Windows.Devices.Midi2.Initialization.hpp`,
/// RC4), which is what the SDK's own clients compile against.
/// `EnsureServiceAvailable` demand-starts the service.
#[interface("8087b303-d551-bce2-1ead-a2500d50c580")]
unsafe trait IMidiClientInitializer: IUnknown {
    fn GetInstalledWindowsMidiServicesSdkVersion(
        &self,
        platform: *mut i32,
        major: *mut u16,
        minor: *mut u16,
        patch: *mut u16,
        source: *mut *mut u16,
        name: *mut *mut u16,
        full: *mut *mut u16,
    ) -> HRESULT;
    fn EnsureServiceAvailable(&self) -> HRESULT;
}

const CLSID_MIDI_CLIENT_INITIALIZER: GUID = GUID::from_u128(0xc3263827_c3b0_bdbd_2500_ce63a3f3f2c3);

struct Runtime {
    /// Held for the life of the process: the activation redirection is
    /// installed by this object and is not needed to be torn down before
    /// the process ends.
    _initializer: IMidiClientInitializer,
    /// The installed SDK's version, for the log.
    version: String,
}

// SAFETY: the initializer is called exactly once, on the thread that
// bootstraps it; afterwards it is only kept alive. The runtime itself is
// free-threaded, as every WinRT class it activates is.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

/// The installed SDK's version, once the runtime is up.
pub fn version() -> Result<String> {
    runtime().map(|runtime| runtime.version.clone())
}

/// Ticks per second of the clock that stamps every message: the system's
/// performance counter, which is what [`clock_now`] reads.
#[allow(dead_code)]
pub fn clock_frequency() -> Result<u64> {
    runtime()?;
    MidiClock::TimestampFrequency().context("reading the MIDI clock frequency")
}

/// The clock's reading now, in the same ticks the messages carry.
#[allow(dead_code)]
pub fn clock_now() -> Result<u64> {
    runtime()?;
    MidiClock::Now().context("reading the MIDI clock")
}

/// The SDK runtime, bootstrapped once per process. `Err` names, in one
/// line, why Windows MIDI Services is not available here.
fn runtime() -> Result<&'static Runtime> {
    RUNTIME
        .get_or_init(|| {
            // The redirection is process-wide; the apartment is whatever
            // this thread already is. RPC_E_CHANGED_MODE on a thread that
            // is already an STA is fine: COM is initialised either way.
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let initializer: IMidiClientInitializer = unsafe {
                CoCreateInstance(&CLSID_MIDI_CLIENT_INITIALIZER, None, CLSCTX_INPROC_SERVER)
            }
            .map_err(|error| {
                format!("the Windows MIDI Services App SDK runtime is not installed ({error})")
            })?;
            let (mut major, mut minor, mut patch) = (0u16, 0u16, 0u16);
            unsafe {
                initializer.GetInstalledWindowsMidiServicesSdkVersion(
                    std::ptr::null_mut(),
                    &mut major,
                    &mut minor,
                    &mut patch,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            }
            .ok()
            .map_err(|error| format!("the SDK runtime did not report a version ({error})"))?;
            unsafe { initializer.EnsureServiceAvailable() }
                .ok()
                .map_err(|error| format!("the Windows MIDI Service is not available ({error})"))?;
            Ok(Runtime {
                _initializer: initializer,
                version: format!("{major}.{minor}.{patch}"),
            })
        })
        .as_ref()
        .map_err(|reason| anyhow::anyhow!("{reason}"))
}

/// One endpoint the service exposes for ordinary messages.
#[derive(Clone, Debug)]
pub struct Endpoint {
    /// The service's display name.
    pub name: String,
    /// The endpoint device id, which is what a connection is opened on.
    pub id: HSTRING,
    /// The sources this endpoint offers: one per group the service
    /// associates with a MIDI 1.0 input port, or one for the whole
    /// endpoint when it has none. Port names are unique across every
    /// endpoint returned together (a repeated one is numbered).
    pub sources: Vec<GroupSource>,
}

/// A source inside an endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSource {
    /// The group whose messages this source is, or `None` for every group.
    pub group: Option<u8>,
    /// The MIDI 1.0 port name the service associates with the group, or
    /// the endpoint's name when there is none. Never prefixed.
    pub port_name: String,
}

impl Endpoint {
    /// The source names this endpoint is selected under.
    pub fn source_names(&self) -> impl Iterator<Item = String> + '_ {
        self.sources
            .iter()
            .map(|source| source_name(&source.port_name))
    }
}

/// The source name an endpoint is selected and saved under.
pub fn source_name(endpoint_name: &str) -> String {
    format!("{NAME_PREFIX}{endpoint_name}")
}

/// The endpoint name inside a source name, if the source is one of ours.
pub fn endpoint_name(source: &str) -> Option<&str> {
    source.strip_prefix(NAME_PREFIX)
}

/// Every endpoint meant for messages, sorted by name. Diagnostic loopbacks,
/// the in-box synth and virtual-device responders are not inputs.
pub fn endpoints() -> Result<Vec<Endpoint>> {
    runtime()?;
    let all = MidiEndpointDeviceInformation::FindAll().context("enumerating UMP endpoints")?;
    let mut endpoints = Vec::new();
    for index in 0..all.Size().context("counting UMP endpoints")? {
        let info = all.GetAt(index).context("reading a UMP endpoint")?;
        if info.EndpointPurpose()? != MidiEndpointDevicePurpose::NormalMessageEndpoint {
            continue;
        }
        let name = info.Name()?.to_string_lossy().trim().to_owned();
        let mut sources = Vec::new();
        let ports = info
            .FindAllAssociatedMidi1PortsForThisEndpoint(Midi1PortFlow::MidiMessageSource)
            .context("reading a UMP endpoint's MIDI 1.0 ports")?;
        for index in 0..ports.Size()? {
            let port = ports.GetAt(index)?;
            sources.push(GroupSource {
                group: Some(port.Group()?.Index()?),
                port_name: port.PortName()?.to_string_lossy().trim().to_owned(),
            });
        }
        sources.sort_by_key(|source| source.group);
        if sources.is_empty() {
            sources.push(GroupSource {
                group: None,
                port_name: name.clone(),
            });
        }
        endpoints.push(Endpoint {
            name,
            id: info.EndpointDeviceId()?,
            sources,
        });
    }
    endpoints.sort_by(|left, right| left.name.cmp(&right.name));
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for source in endpoints
        .iter_mut()
        .flat_map(|endpoint| &mut endpoint.sources)
    {
        let count = seen.entry(source.port_name.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            source.port_name = format!("{} ({})", source.port_name, count);
        }
    }
    Ok(endpoints)
}

/// The source names of every endpoint, or nothing at all when the runtime
/// is not here -- said once, so the log carries the reason and not a
/// heartbeat of it.
pub fn discover() -> Vec<String> {
    static ANNOUNCED: OnceLock<()> = OnceLock::new();
    match endpoints() {
        Ok(endpoints) => endpoints.iter().flat_map(Endpoint::source_names).collect(),
        Err(error) => {
            ANNOUNCED.get_or_init(|| eprintln!("DESKTOP_UMP_UNAVAILABLE reason={error:#}"));
            Vec::new()
        }
    }
}

/// A session with the service; connections are opened through it and
/// closed by it.
pub struct Transport {
    session: MidiSession,
}

impl Transport {
    pub fn open() -> Result<Self> {
        runtime()?;
        let session =
            MidiSession::Create(&HSTRING::from("RackForge")).context("opening a UMP session")?;
        Ok(Self { session })
    }

    /// Opens `endpoint` and hands every packet it receives -- one to four
    /// words, and the service's timestamp for it -- to `on_words`, on the
    /// service's thread. Dropping the connection closes it.
    pub fn connect(
        &self,
        endpoint: &Endpoint,
        on_words: impl Fn(&[u32], u64) + Send + Sync + 'static,
    ) -> Result<Connection> {
        let connection = self
            .session
            .CreateEndpointConnection(&endpoint.id)
            .with_context(|| format!("connecting UMP endpoint {:?}", endpoint.name))?;
        let handler = TypedEventHandler::<
            IMidiMessageReceivedEventSource,
            MidiMessageReceivedEventArgs,
        >::new(move |_, args: Ref<MidiMessageReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let mut words = [0u32; 4];
                let [first, second, third, fourth] = &mut words;
                let count = args.FillWords(first, second, third, fourth)?;
                let timestamp = args.Timestamp()?;
                on_words(&words[..usize::from(count).min(words.len())], timestamp);
            }
            Ok(())
        });
        let token = connection
            .MessageReceived(&handler)
            .context("subscribing to UMP messages")?;
        if !connection
            .Open()
            .with_context(|| format!("opening UMP endpoint {:?}", endpoint.name))?
        {
            let _ = connection.RemoveMessageReceived(token);
            bail!("UMP endpoint {:?} refused to open", endpoint.name);
        }
        Ok(Connection {
            session: self.session.clone(),
            connection,
            token,
        })
    }
}

/// An open endpoint. Messages arrive through the closure given to
/// [`Transport::connect`] until this is dropped.
pub struct Connection {
    session: MidiSession,
    connection: MidiEndpointConnection,
    token: i64,
}

impl Connection {
    /// Sends one two-word packet, now (`timestamp` 0) or at a clock reading
    /// the service schedules it for; for the loopback proofs below.
    #[cfg(test)]
    fn send_two_words(&self, timestamp: u64, first: u32, second: u32) -> Result<()> {
        self.connection
            .SendSingleMessageWords2(timestamp, first, second)
            .context("sending a UMP message")?;
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.connection.RemoveMessageReceived(self.token);
        if let Ok(id) = self.connection.ConnectionId() {
            let _ = self.session.DisconnectEndpointConnection(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Needs Windows MIDI Services with its App SDK runtime installed and
    /// the default loopback pair it creates: what goes into A comes out of
    /// B. A MIDI 2.0 note-on with a full 16-bit velocity crosses the service
    /// whole and arrives as a host packet carrying that width.
    #[test]
    #[ignore]
    fn a_wide_note_survives_the_loopback() {
        eprintln!("step: bootstrap sdk={}", version().unwrap());
        eprintln!("step: session");
        let transport = Transport::open().unwrap();
        eprintln!("step: enumerate");
        let endpoints = endpoints().unwrap();
        eprintln!(
            "step: endpoints={:?}",
            endpoints.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
        );
        let a = endpoints
            .iter()
            .find(|endpoint| endpoint.name.contains("Loopback (A)"))
            .expect("Default App Loopback (A)");
        let b = endpoints
            .iter()
            .find(|endpoint| endpoint.name.contains("Loopback (B)"))
            .expect("Default App Loopback (B)");
        eprintln!(
            "step: sources={:?}",
            endpoints
                .iter()
                .flat_map(|endpoint| endpoint
                    .sources
                    .iter()
                    .map(|s| (s.group, s.port_name.clone())))
                .collect::<Vec<_>>()
        );
        let (sender, receiver) = mpsc::channel();
        let _receiving = transport
            .connect(b, move |words, timestamp| {
                let _ = sender.send((words.to_vec(), timestamp));
            })
            .unwrap();
        eprintln!("step: connected B");
        let sending = transport.connect(a, |_, _| {}).unwrap();
        eprintln!("step: connected A");
        let before = clock_now().unwrap();
        sending.send_two_words(0, 0x4090_3c00, 0xffff_0000).unwrap();
        eprintln!("step: sent");
        let (words, timestamp) = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("a packet out of loopback B");
        assert_eq!(words, [0x4090_3c00, 0xffff_0000]);
        // Stamped by the same clock we read, and no earlier than the send.
        let after = clock_now().unwrap();
        assert!(
            before <= timestamp && timestamp <= after,
            "{before} <= {timestamp} <= {after}"
        );
        let mut packets = Vec::new();
        rackforge_core::ump::read_stream(
            &words,
            0,
            &mut |_, packet| packets.push(packet),
            &mut |_, _| {},
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].data, [0x90, 60, 127]);
        assert_eq!(packets[0].wide, Some(0xffff));
    }

    /// Two notes sent five milliseconds apart -- sent NOW, as a keyboard
    /// sends them, not scheduled -- come out of the loopback five
    /// milliseconds apart by their timestamps, which is the spacing the
    /// audio thread turns into a frame offset, whatever the delivery jitter.
    /// (A future timestamp on send engages the service's scheduler, whose
    /// first release is coarse; that is a different feature, not this proof.)
    #[test]
    #[ignore]
    fn timestamps_keep_the_spacing_between_notes() {
        let transport = Transport::open().unwrap();
        let endpoints = endpoints().unwrap();
        let a = endpoints
            .iter()
            .find(|endpoint| endpoint.name.contains("Loopback (A)"))
            .expect("Default App Loopback (A)");
        let b = endpoints
            .iter()
            .find(|endpoint| endpoint.name.contains("Loopback (B)"))
            .expect("Default App Loopback (B)");
        let (sender, receiver) = mpsc::channel();
        let _receiving = transport
            .connect(b, move |_, timestamp| {
                let _ = sender.send(timestamp);
            })
            .unwrap();
        let sending = transport.connect(a, |_, _| {}).unwrap();
        let frequency = clock_frequency().unwrap();
        let millisecond = frequency / 1000;
        let first_sent = clock_now().unwrap();
        sending.send_two_words(0, 0x4090_3c00, 0x8000_0000).unwrap();
        while clock_now().unwrap() < first_sent + 5 * millisecond {
            std::hint::spin_loop();
        }
        let second_sent = clock_now().unwrap();
        sending.send_two_words(0, 0x4090_3e00, 0x8000_0000).unwrap();
        let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let second = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let ms = |ticks: f64| ticks * 1000.0 / frequency as f64;
        let sent_spacing = ms(second_sent as f64 - first_sent as f64);
        let stamped_spacing = ms(second as f64 - first as f64);
        eprintln!(
            "step: sent {sent_spacing:.3} ms apart, stamped {stamped_spacing:.3} ms apart, first stamped {:.3} ms after send",
            ms(first as f64 - first_sent as f64)
        );
        assert!(
            (stamped_spacing - sent_spacing).abs() < 1.0,
            "stamped {stamped_spacing} ms vs sent {sent_spacing} ms"
        );
    }

    #[test]
    fn source_names_carry_the_transport() {
        assert_eq!(
            source_name("Default App Loopback (B)"),
            "UMP: Default App Loopback (B)"
        );
        assert_eq!(
            endpoint_name("UMP: KL Essential 61 mk3"),
            Some("KL Essential 61 mk3")
        );
        assert_eq!(endpoint_name("KeyLab Essential 61 mk3"), None);
    }
}
