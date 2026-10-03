//! Windows MIDI Services where there is none: the same surface as `input`,
//! finding no endpoint and opening nothing. The desktop host keeps one MIDI
//! code path on every platform -- `midir` for ports, this for UMP endpoints --
//! and on Linux every MIDI port comes through `midir` (ALSA sequencer).

use anyhow::{Result, bail};

/// What a Windows MIDI Services endpoint is called as a RackForge source.
pub const NAME_PREFIX: &str = "UMP: ";

const UNAVAILABLE: &str = "Windows MIDI Services exists only on Windows";

pub fn version() -> Result<String> {
    bail!(UNAVAILABLE)
}

#[allow(dead_code)]
pub fn clock_frequency() -> Result<u64> {
    bail!(UNAVAILABLE)
}

#[allow(dead_code)]
pub fn clock_now() -> Result<u64> {
    bail!(UNAVAILABLE)
}

/// One endpoint the service exposes for ordinary messages; never produced
/// here.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub name: String,
    pub id: String,
    pub sources: Vec<GroupSource>,
}

/// A source inside an endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSource {
    pub group: Option<u8>,
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

/// The endpoint name inside a source name, if the source is one of ours. A
/// session saved on Windows may carry such a name; it simply never connects.
pub fn endpoint_name(source: &str) -> Option<&str> {
    source.strip_prefix(NAME_PREFIX)
}

/// No endpoints: there is no service.
pub fn endpoints() -> Result<Vec<Endpoint>> {
    bail!(UNAVAILABLE)
}

/// No source names, and nothing to announce: the absence is the platform's,
/// not a fault.
pub fn discover() -> Vec<String> {
    Vec::new()
}

/// A session with the service; it never opens here.
pub struct Transport;

impl Transport {
    pub fn open() -> Result<Self> {
        bail!(UNAVAILABLE)
    }

    pub fn connect(
        &self,
        endpoint: &Endpoint,
        _on_words: impl Fn(&[u32], u64) + Send + Sync + 'static,
    ) -> Result<Connection> {
        bail!(
            "{UNAVAILABLE}: cannot open UMP endpoint {:?}",
            endpoint.name
        )
    }
}

/// An open endpoint; never produced here.
pub struct Connection;
