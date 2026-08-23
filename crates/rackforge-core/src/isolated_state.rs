use crate::{LoadedPlugin, PluginInstance};
use anyhow::{Context, Result, bail};
use rackforge_control_api::PluginParameterValue;
use rackforge_plugin_api::{
    Capability, ParameterDescriptor, ParameterKind, ParameterSchema, PluginStateReference,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A short-lived portable instance used to inspect or mutate immutable state.
///
/// It is never activated or published to the audio graph. Dropping this value
/// destroys the instance and leaves PLAY and every other Rack Slot untouched.
pub struct IsolatedPluginStateEditor<'a> {
    plugin: &'a LoadedPlugin,
    instance: PluginInstance<'a>,
}

impl<'a> IsolatedPluginStateEditor<'a> {
    pub fn open(
        plugin: &'a LoadedPlugin,
        resource_overrides: &BTreeMap<String, PathBuf>,
        bytes: &[u8],
    ) -> Result<Self> {
        let mut instance = plugin
            .create_instance_with_resource_overrides(resource_overrides)
            .context("creating isolated plugin state instance")?;
        instance
            .load_state(bytes)
            .context("loading isolated plugin state")?;
        Ok(Self { plugin, instance })
    }

    pub fn parameters(&mut self) -> Result<(ParameterSchema, Vec<PluginParameterValue>)> {
        let schema = self.plugin.parameters().clone();
        let values = schema
            .parameters
            .iter()
            .map(|parameter| {
                self.instance
                    .get_parameter(parameter.index)
                    .with_context(|| format!("reading plugin parameter {}", parameter.id))
                    .map(|value| PluginParameterValue {
                        index: parameter.index,
                        value,
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((schema, values))
    }

    pub fn set_parameter(&mut self, parameter_index: u32, value: f64) -> Result<f64> {
        let parameter = validate_parameter_write(self.plugin.parameters(), parameter_index, value)?;
        self.instance
            .set_parameter(parameter_index, value)
            .with_context(|| format!("setting plugin parameter {}", parameter.id))?;
        self.instance
            .get_parameter(parameter_index)
            .with_context(|| format!("reading canonical plugin parameter {}", parameter.id))
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        self.instance
            .save_state()
            .context("saving isolated plugin state")
    }
}

/// Reads the complete public parameter surface from an active plugin instance.
///
/// Hosts use this for live instances while [`IsolatedPluginStateEditor`] uses
/// the same representation for immutable Rack Slot state.
pub fn plugin_parameters(
    plugin: &LoadedPlugin,
    instance: &mut PluginInstance<'_>,
) -> Result<(ParameterSchema, Vec<PluginParameterValue>)> {
    let schema = plugin.parameters().clone();
    let values = schema
        .parameters
        .iter()
        .map(|parameter| {
            instance
                .get_parameter(parameter.index)
                .with_context(|| format!("reading plugin parameter {}", parameter.id))
                .map(|value| PluginParameterValue {
                    index: parameter.index,
                    value,
                })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((schema, values))
}

/// Applies one validated write to an active plugin instance and returns the
/// canonical value reported by the plugin afterwards.
pub fn set_plugin_parameter(
    plugin: &LoadedPlugin,
    instance: &mut PluginInstance<'_>,
    parameter_index: u32,
    value: f64,
) -> Result<f64> {
    let parameter = validate_parameter_write(plugin.parameters(), parameter_index, value)?;
    instance
        .set_parameter(parameter_index, value)
        .with_context(|| format!("setting plugin parameter {}", parameter.id))?;
    instance
        .get_parameter(parameter_index)
        .with_context(|| format!("reading canonical plugin parameter {}", parameter.id))
}

pub fn validate_state_reference(
    plugin: &LoadedPlugin,
    reference: &PluginStateReference,
) -> Result<()> {
    reference
        .validate()
        .context("validating plugin state reference")?;
    let manifest = plugin.manifest();
    if !manifest.capabilities.contains(&Capability::State) {
        bail!(
            "plugin {} does not support complete state snapshots",
            manifest.id
        );
    }
    if reference.plugin_id != manifest.id {
        bail!(
            "plugin state belongs to {}, not {}",
            reference.plugin_id,
            manifest.id
        );
    }
    if reference.plugin_version != manifest.version {
        bail!(
            "plugin state version {} is incompatible with installed plugin version {}",
            reference.plugin_version,
            manifest.version
        );
    }
    if reference.state_version != manifest.state_version {
        bail!(
            "plugin state schema {} is incompatible with installed state schema {}",
            reference.state_version,
            manifest.state_version
        );
    }
    Ok(())
}

pub fn parameter_value_is_valid(kind: &ParameterKind, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match kind {
        ParameterKind::Float {
            minimum, maximum, ..
        }
        | ParameterKind::Meter {
            minimum, maximum, ..
        } => value >= *minimum && value <= *maximum,
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => {
            value.fract() == 0.0
                && value >= *minimum as f64
                && value <= *maximum as f64
                && ((value as i64 - minimum) % step == 0)
        }
        ParameterKind::Boolean { .. } => value == 0.0 || value == 1.0,
        ParameterKind::Enum { choices, .. } => {
            value.fract() == 0.0
                && value >= 0.0
                && value <= u32::MAX as f64
                && choices.iter().any(|choice| choice.value == value as u32)
        }
        ParameterKind::Trigger => value == 0.0 || value == 1.0,
    }
}

pub fn validate_parameter_write(
    schema: &ParameterSchema,
    parameter_index: u32,
    value: f64,
) -> Result<&ParameterDescriptor> {
    let parameter = schema
        .parameters
        .iter()
        .find(|parameter| parameter.index == parameter_index)
        .with_context(|| format!("unknown plugin parameter {parameter_index}"))?;
    if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
        bail!("plugin parameter {} is read-only", parameter.id);
    }
    if !parameter_value_is_valid(&parameter.kind, value) {
        bail!(
            "invalid value {value} for plugin parameter {}",
            parameter.id
        );
    }
    Ok(parameter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PluginPackage, PluginStateStore};
    use rackforge_plugin_api::{EnumChoice, ParameterFlags};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SERIAL: AtomicU64 = AtomicU64::new(1);

    const STATEFUL_GAIN: &str = r#"
        (module
          (memory (export "memory") 1)
          (global $gain (mut f64) (f64.const 1))
          (func (export "rackforge_abi_version") (result i32) i32.const 65537)
          (func (export "rackforge_input_ptr") (result i32) i32.const 0)
          (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
          (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
          (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
          (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
          (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
          (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
          (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
          (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
          (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
          (func (export "rackforge_initialize") (result i32) i32.const 0)
          (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
          (func (export "rackforge_set_parameter") (param i32) (param $value f64) (result i32)
            local.get $value global.set $gain i32.const 0)
          (func (export "rackforge_get_parameter") (param i32) (result f64)
            global.get $gain)
          (func (export "rackforge_reset") (result i32) i32.const 0)
          (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
          (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
          (func (export "rackforge_resource_end") (result i32) i32.const -3)
          (func (export "rackforge_load_preset") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_save_state") (result i32)
            i32.const 8192 global.get $gain f64.store i32.const 8)
          (func (export "rackforge_load_state") (param $length i32) (result i32)
            local.get $length i32.const 8 i32.ne
            if (result i32)
              i32.const -1
            else
              i32.const 8192 f64.load global.set $gain i32.const 0
            end)
          (func (export "rackforge_process") (param i32 i32 i32 i32 i32) (result i32)
            i32.const 0))
    "#;

    fn parameter(kind: ParameterKind, read_only: bool) -> ParameterDescriptor {
        ParameterDescriptor {
            index: 9,
            id: "test".into(),
            name: "Test".into(),
            page: "main".into(),
            group: None,
            order: 0,
            kind,
            flags: ParameterFlags {
                read_only,
                ..ParameterFlags::default()
            },
            suggested_control: Default::default(),
        }
    }

    fn schema(parameter: ParameterDescriptor) -> ParameterSchema {
        ParameterSchema {
            schema_version: 1,
            pages: Vec::new(),
            parameters: vec![parameter],
            semantic_controls: Vec::new(),
        }
    }

    #[test]
    fn validates_float_boolean_enum_and_read_only_parameter_writes() {
        let float = schema(parameter(
            ParameterKind::Float {
                minimum: -1.0,
                maximum: 1.0,
                default: 0.0,
                step: 0.01,
                unit: None,
            },
            false,
        ));
        assert!(validate_parameter_write(&float, 9, 0.75).is_ok());
        assert!(validate_parameter_write(&float, 9, 1.25).is_err());
        assert!(validate_parameter_write(&float, 9, f64::NAN).is_err());
        assert!(validate_parameter_write(&float, 9, f64::INFINITY).is_err());
        assert!(validate_parameter_write(&float, 10, 0.0).is_err());

        let boolean = schema(parameter(ParameterKind::Boolean { default: false }, false));
        assert!(validate_parameter_write(&boolean, 9, 1.0).is_ok());
        assert!(validate_parameter_write(&boolean, 9, 0.5).is_err());

        let integer = schema(parameter(
            ParameterKind::Integer {
                minimum: 2,
                maximum: 10,
                default: 2,
                step: 2,
                unit: None,
            },
            false,
        ));
        assert!(validate_parameter_write(&integer, 9, 8.0).is_ok());
        assert!(validate_parameter_write(&integer, 9, 7.0).is_err());
        assert!(validate_parameter_write(&integer, 9, 8.5).is_err());

        let enumeration = schema(parameter(
            ParameterKind::Enum {
                default: 3,
                choices: vec![
                    EnumChoice {
                        value: 3,
                        name: "Three".into(),
                    },
                    EnumChoice {
                        value: 8,
                        name: "Eight".into(),
                    },
                ],
            },
            false,
        ));
        assert!(validate_parameter_write(&enumeration, 9, 8.0).is_ok());
        assert!(validate_parameter_write(&enumeration, 9, 4.0).is_err());

        let trigger = schema(parameter(ParameterKind::Trigger, false));
        assert!(validate_parameter_write(&trigger, 9, 1.0).is_ok());
        assert!(validate_parameter_write(&trigger, 9, 0.0).is_ok());
        assert!(validate_parameter_write(&trigger, 9, 2.0).is_err());

        let read_only = schema(parameter(ParameterKind::Boolean { default: false }, true));
        assert!(validate_parameter_write(&read_only, 9, 1.0).is_err());
    }

    fn stateful_plugin_fixture() -> (PathBuf, LoadedPlugin) {
        let root = std::env::temp_dir().join(format!(
            "rackforge-isolated-state-{}-{}",
            std::process::id(),
            TEST_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("metadata")).unwrap();
        fs::write(
            root.join("rackforge-plugin.toml"),
            r#"schema_version = 1
id = "org.rackforge.test-state"
name = "State Test"
vendor = "RackForge"
version = "1.2.3"
kind = "instrument"
state_version = 4
capabilities = ["audio_output", "state"]

[api]
major = 1
minor = 5

[component]
abi = "wasm-v1"
path = "component.wasm"
runtime_descriptor = "metadata/runtime.json"
parameter_schema = "metadata/parameters.json"
preset_catalog = "metadata/presets.json"
"#,
        )
        .unwrap();
        fs::write(
            root.join("metadata/runtime.json"),
            r#"{"schema_version":1,"id":"org.rackforge.test-state","version":"1.2.3","state_version":4}"#,
        )
        .unwrap();
        fs::write(
            root.join("metadata/parameters.json"),
            r#"{
              "schema_version":1,
              "pages":[{"id":"main","name":"Main","order":0}],
              "parameters":[
                {"index":0,"id":"gain","name":"Gain","page":"main","order":0,
                 "kind":{"type":"float","minimum":0.0,"maximum":1.0,"default":1.0,"step":0.01},
                 "flags":{},"suggested_control":"knob"},
                {"index":1,"id":"enabled","name":"Enabled","page":"main","order":1,
                 "kind":{"type":"boolean","default":true},"flags":{},"suggested_control":"toggle"},
                {"index":2,"id":"mode","name":"Mode","page":"main","order":2,
                 "kind":{"type":"enum","default":0,"choices":[{"value":0,"name":"A"},{"value":2,"name":"B"}]},
                 "flags":{},"suggested_control":"list"},
                {"index":3,"id":"meter","name":"Meter","page":"main","order":3,
                 "kind":{"type":"meter","minimum":0.0,"maximum":1.0},
                 "flags":{"read_only":true},"suggested_control":"meter"}
              ]
            }"#,
        )
        .unwrap();
        fs::write(
            root.join("metadata/presets.json"),
            r#"{"schema_version":1,"banks":[],"presets":[]}"#,
        )
        .unwrap();
        fs::write(
            root.join("component.wasm"),
            wat::parse_str(STATEFUL_GAIN).unwrap(),
        )
        .unwrap();
        let package = PluginPackage::open(&root).unwrap();
        // SAFETY: the fixture is a portable wasm-v1 component assembled above.
        let plugin = unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), None) }.unwrap();
        (root, plugin)
    }

    fn state_bytes(plugin: &LoadedPlugin, value: f64) -> Vec<u8> {
        let mut instance = plugin
            .create_instance_with_resource_overrides(&BTreeMap::new())
            .unwrap();
        instance.set_parameter(0, value).unwrap();
        instance.save_state().unwrap()
    }

    #[test]
    fn live_parameter_helpers_read_schema_and_return_the_canonical_write() {
        let (root, plugin) = stateful_plugin_fixture();
        let mut instance = plugin
            .create_instance_with_resource_overrides(&BTreeMap::new())
            .unwrap();
        instance.set_parameter(0, 0.25).unwrap();

        let (schema, values) = plugin_parameters(&plugin, &mut instance).unwrap();
        assert_eq!(schema.parameters.len(), 4);
        assert_eq!(
            values[0],
            PluginParameterValue {
                index: 0,
                value: 0.25
            }
        );
        assert_eq!(
            set_plugin_parameter(&plugin, &mut instance, 0, 0.75).unwrap(),
            0.75
        );
        assert_eq!(instance.get_parameter(0).unwrap(), 0.75);

        drop(instance);
        drop(plugin);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn isolated_edits_are_immutable_and_do_not_touch_play_or_sibling_slots() {
        let (root, plugin) = stateful_plugin_fixture();
        let mut store = PluginStateStore::new(None).unwrap();
        let manifest = plugin.manifest();
        let original_bytes = state_bytes(&plugin, 0.4);
        let original = store
            .put(
                &manifest.id,
                &manifest.version,
                manifest.state_version,
                Some("factory.warm".into()),
                &original_bytes,
            )
            .unwrap();
        let sibling_bytes = state_bytes(&plugin, 0.6);
        let sibling = store
            .put(
                &manifest.id,
                &manifest.version,
                manifest.state_version,
                None,
                &sibling_bytes,
            )
            .unwrap();
        let mut play = plugin
            .create_instance_with_resource_overrides(&BTreeMap::new())
            .unwrap();
        play.set_parameter(0, 0.2).unwrap();

        let mut editor =
            IsolatedPluginStateEditor::open(&plugin, &BTreeMap::new(), &original_bytes).unwrap();
        assert_eq!(editor.set_parameter(0, 0.8).unwrap(), 0.8);
        let edited_bytes = editor.save_state().unwrap();
        drop(editor);
        let edited = store
            .put(
                &manifest.id,
                &manifest.version,
                manifest.state_version,
                original.selected_sound_id.clone(),
                &edited_bytes,
            )
            .unwrap();

        assert_ne!(edited.blob_sha256, original.blob_sha256);
        assert_eq!(edited.selected_sound_id.as_deref(), Some("factory.warm"));
        assert_eq!(store.read(&original).unwrap(), original_bytes);
        assert_eq!(store.read(&sibling).unwrap(), sibling_bytes);
        assert_eq!(play.get_parameter(0).unwrap(), 0.2);
        let mut sibling_editor =
            IsolatedPluginStateEditor::open(&plugin, &BTreeMap::new(), &sibling_bytes).unwrap();
        assert_eq!(sibling_editor.parameters().unwrap().1[0].value, 0.6);
        drop(sibling_editor);
        drop(play);
        drop(plugin);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incompatible_references_and_missing_blobs_are_rejected() {
        let (root, plugin) = stateful_plugin_fixture();
        let mut store = PluginStateStore::new(None).unwrap();
        let manifest = plugin.manifest();
        let state = store
            .put(
                &manifest.id,
                &manifest.version,
                manifest.state_version,
                None,
                &state_bytes(&plugin, 0.5),
            )
            .unwrap();

        let mut wrong_plugin = state.clone();
        wrong_plugin.plugin_id = "org.rackforge.somewhere-else".into();
        assert!(validate_state_reference(&plugin, &wrong_plugin).is_err());
        let mut wrong_version = state.clone();
        wrong_version.plugin_version = "9.0.0".into();
        assert!(validate_state_reference(&plugin, &wrong_version).is_err());
        let mut wrong_schema = state.clone();
        wrong_schema.state_version += 1;
        assert!(validate_state_reference(&plugin, &wrong_schema).is_err());
        let mut missing = state;
        missing.blob_sha256 = "f".repeat(64);
        assert!(store.read(&missing).is_err());

        drop(plugin);
        fs::remove_dir_all(root).unwrap();
    }
}
