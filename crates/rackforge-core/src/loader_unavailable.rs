//! Stand-in for the native plugin loader on hosts without dynamic libraries.
//!
//! The browser host runs sandboxed `wasm-v1` components only: a page cannot
//! `dlopen` a `.so` or a `.dll`, and would not be allowed to trust one if it
//! could. Rather than scattering `cfg` attributes through [`crate::hosted`],
//! this module supplies the same two types with the same signatures, made
//! uninhabited so the compiler proves the native path is never taken. Loading
//! a native package reports why it was refused; every other method is
//! unreachable by construction rather than by panic.

use crate::PluginPackage;
use anyhow::{Result, bail};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use rackforge_plugin_api::{
    ParameterSchema, PluginManifest, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, ProgramEditorView, ProgramFieldEditRequest, RuntimeDescriptor,
    SurfaceActivationRequest, SurfaceActivationResponse,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Uninhabited: no value of this type can exist, so neither can a loaded
/// native plugin.
enum NoNativeRuntime {}

pub(crate) struct NativeLoadedPlugin(NoNativeRuntime);

impl NativeLoadedPlugin {
    /// Always refuses. Native packages need a dynamic linker the host does not
    /// have.
    ///
    /// # Safety
    ///
    /// Trivially satisfied: nothing is loaded and no foreign code runs.
    pub unsafe fn load(
        package: &PluginPackage,
        _binary_override: Option<&Path>,
        _resource_overrides: &BTreeMap<String, PathBuf>,
        _data_root: Option<&Path>,
    ) -> Result<Self> {
        bail!(
            "plugin {} needs the native runtime, which this host does not provide; \
             install its portable wasm-v1 package instead",
            package.manifest().id
        )
    }

    pub fn manifest(&self) -> &PluginManifest {
        match self.0 {}
    }

    pub fn descriptor(&self) -> &RuntimeDescriptor {
        match self.0 {}
    }

    pub fn parameters(&self) -> &ParameterSchema {
        match self.0 {}
    }

    pub fn presets(&self) -> &PresetCatalog {
        match self.0 {}
    }

    pub fn create_instance(&self) -> Result<NativePluginInstance<'_>> {
        match self.0 {}
    }
}

pub(crate) struct NativePluginInstance<'plugin> {
    plugin: &'plugin NativeLoadedPlugin,
}

impl NativePluginInstance<'_> {
    fn absent(&self) -> ! {
        match self.plugin.0 {}
    }

    pub fn supports_program_editing(&self) -> bool {
        self.absent()
    }

    pub fn activate_surface(
        &mut self,
        _request: &SurfaceActivationRequest,
    ) -> Result<SurfaceActivationResponse> {
        self.absent()
    }

    pub fn begin_program_edit(&mut self, _request: &ProgramEditRequest) -> Result<PreparedProgram> {
        self.absent()
    }

    pub fn prepare_program_save(&mut self, _document: &ProgramDocument) -> Result<PreparedProgram> {
        self.absent()
    }

    pub fn install_program(&mut self, _prepared: &PreparedProgram) -> Result<()> {
        self.absent()
    }

    pub fn preview_program(&mut self, _prepared: &PreparedProgram) -> Result<bool> {
        self.absent()
    }

    pub fn program_editor_view(
        &mut self,
        _document: &ProgramDocument,
    ) -> Result<ProgramEditorView> {
        self.absent()
    }

    pub fn apply_program_edit(
        &mut self,
        _request: &ProgramFieldEditRequest,
    ) -> Result<PreparedProgram> {
        self.absent()
    }

    pub fn preset_catalog(&self) -> Result<PresetCatalog> {
        self.absent()
    }

    pub fn activate(
        &mut self,
        _sample_rate: f64,
        _maximum_frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) -> Result<()> {
        self.absent()
    }

    pub fn deactivate(&mut self) -> Result<()> {
        self.absent()
    }

    pub fn reset(&mut self) -> Result<()> {
        self.absent()
    }

    pub fn set_parameter(&mut self, _index: u32, _value: f64) -> Result<()> {
        self.absent()
    }

    pub fn get_parameter(&self, _index: u32) -> Result<f64> {
        self.absent()
    }

    pub fn save_state(&self) -> Result<Vec<u8>> {
        self.absent()
    }

    pub fn load_state(&mut self, _bytes: &[u8]) -> Result<()> {
        self.absent()
    }

    pub fn load_preset(&mut self, _preset_id: &str) -> Result<()> {
        self.absent()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_interleaved(
        &mut self,
        _input: &[f32],
        _output: &mut [f32],
        _frames: u32,
        _input_channels: u32,
        _output_channels: u32,
        _midi_events: &[MidiEventV1],
        _parameter_events: &[ParameterEventV1],
    ) -> Result<()> {
        self.absent()
    }
}
