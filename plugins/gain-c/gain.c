/* A complete RackForge instrument in freestanding C.
 *
 * This exists to prove a claim docs/PLUGIN_ABI.md makes: `wasm-v1` is a plain
 * WebAssembly ABI, and nothing about it is specific to Rust. There is no SDK
 * here, no libc, and no generated code -- only the exports the host looks up
 * and five buffers in this module's own linear memory.
 *
 * It is a gain: audio in, audio out, scaled by a parameter that can be set
 * from the host or moved by MIDI controller 7, the channel volume that every
 * keyboard's slider already sends.
 *
 * Build it with any clang that has the wasm32 target:
 *
 *   clang --target=wasm32 -nostdlib -O2 \
 *         -Wl,--no-entry -Wl,--export-memory \
 *         -o gain.wasm gain.c
 *
 * `-nostdlib` is what keeps the module free of imports, which the host
 * requires: a plugin that imports anything at all is refused before it runs.
 */

/* ---------------------------------------------------------------- status */

#define RF_OK 0
#define RF_INVALID_ARGUMENT (-1)
#define RF_UNKNOWN_PARAMETER (-2)
#define RF_INVALID_STATE (-3)

#define RF_ABI_VERSION 0x00010001

#define RF_EXPORT(name) __attribute__((export_name(name)))

/* --------------------------------------------------------------- buffers
 *
 * Capacities are what this plugin can survive, not what it will be given. The
 * host reads each address once and validates it: aligned to the element,
 * inside linear memory, and never overlapping another buffer. Audio wants
 * four-byte alignment, MIDI and parameter records want eight.
 */

#define RF_MAX_FRAMES 1024
#define RF_MAX_CHANNELS 2
#define RF_MAX_SAMPLES (RF_MAX_FRAMES * RF_MAX_CHANNELS)
#define RF_MAX_MIDI_EVENTS 128
#define RF_MAX_PARAMETER_EVENTS 128
#define RF_TRANSFER_BYTES 1024

static float rf_input[RF_MAX_SAMPLES];
static float rf_output[RF_MAX_SAMPLES];
static _Alignas(8) unsigned char rf_midi[RF_MAX_MIDI_EVENTS * 8];
static _Alignas(8) unsigned char rf_parameters[RF_MAX_PARAMETER_EVENTS * 16];
static unsigned char rf_transfer[RF_TRANSFER_BYTES];

static double rf_gain = 1.0;
static int rf_prepared = 0;

/* An address in this module's linear memory, as the i32 the host expects. */
#define RF_ADDRESS(buffer) ((int)(__UINTPTR_TYPE__)(buffer))

RF_EXPORT("rackforge_input_ptr") int rf_input_ptr(void) { return RF_ADDRESS(rf_input); }
RF_EXPORT("rackforge_output_ptr") int rf_output_ptr(void) { return RF_ADDRESS(rf_output); }
RF_EXPORT("rackforge_midi_ptr") int rf_midi_ptr(void) { return RF_ADDRESS(rf_midi); }
RF_EXPORT("rackforge_parameter_ptr") int rf_parameter_ptr(void) { return RF_ADDRESS(rf_parameters); }
RF_EXPORT("rackforge_transfer_ptr") int rf_transfer_ptr(void) { return RF_ADDRESS(rf_transfer); }

RF_EXPORT("rackforge_capacity_input_samples") int rf_capacity_input(void) { return RF_MAX_SAMPLES; }
RF_EXPORT("rackforge_capacity_output_samples") int rf_capacity_output(void) { return RF_MAX_SAMPLES; }
RF_EXPORT("rackforge_capacity_midi_events") int rf_capacity_midi(void) { return RF_MAX_MIDI_EVENTS; }
RF_EXPORT("rackforge_capacity_parameter_events") int rf_capacity_parameters(void) {
    return RF_MAX_PARAMETER_EVENTS;
}
RF_EXPORT("rackforge_capacity_transfer_bytes") int rf_capacity_transfer(void) {
    return RF_TRANSFER_BYTES;
}

/* -------------------------------------------------------------- instance */

RF_EXPORT("rackforge_abi_version") int rf_abi_version(void) { return RF_ABI_VERSION; }

RF_EXPORT("rackforge_initialize") int rf_initialize(void) {
    rf_gain = 1.0;
    rf_prepared = 0;
    return RF_OK;
}

RF_EXPORT("rackforge_prepare")
int rf_prepare(double sample_rate, int maximum_frames, int input_channels, int output_channels) {
    if (!(sample_rate > 0.0) || maximum_frames <= 0 || maximum_frames > RF_MAX_FRAMES ||
        input_channels < 0 || input_channels > RF_MAX_CHANNELS || output_channels < 0 ||
        output_channels > RF_MAX_CHANNELS) {
        return RF_INVALID_ARGUMENT;
    }
    rf_prepared = 1;
    return RF_OK;
}

RF_EXPORT("rackforge_reset") int rf_reset(void) {
    /* A gain has no voices and no tail, so there is nothing to silence. */
    return RF_OK;
}

/* ------------------------------------------------------------ parameters */

RF_EXPORT("rackforge_set_parameter") int rf_set_parameter(int index, double value) {
    if (index != 0) {
        return RF_UNKNOWN_PARAMETER;
    }
    if (!(value >= 0.0) || value > 4.0) {
        return RF_INVALID_ARGUMENT;
    }
    rf_gain = value;
    return RF_OK;
}

RF_EXPORT("rackforge_get_parameter") double rf_get_parameter(int index) {
    /* No error channel here, so an unknown index answers with a safe value
     * rather than something the host would have to interpret. */
    return index == 0 ? rf_gain : 0.0;
}

/* ------------------------------------- state, presets, external resources
 *
 * Declined, and declining is a supported answer: the host handles -3 and
 * carries on. A plugin only implements what it actually has.
 */

RF_EXPORT("rackforge_save_state") int rf_save_state(void) { return RF_INVALID_STATE; }
RF_EXPORT("rackforge_load_state") int rf_load_state(int length) {
    (void)length;
    return RF_INVALID_STATE;
}
RF_EXPORT("rackforge_load_preset") int rf_load_preset(int length) {
    (void)length;
    return RF_INVALID_STATE;
}
RF_EXPORT("rackforge_resource_begin") int rf_resource_begin(int id_length, long long total_bytes) {
    (void)id_length;
    (void)total_bytes;
    return RF_INVALID_STATE;
}
RF_EXPORT("rackforge_resource_write") int rf_resource_write(long long offset, int length) {
    (void)offset;
    (void)length;
    return RF_INVALID_STATE;
}
RF_EXPORT("rackforge_resource_end") int rf_resource_end(void) { return RF_INVALID_STATE; }

/* ------------------------------------------------------------------ audio */

/* One MIDI 1.0 event is a little-endian 64-bit word: the frame in the low 32
 * bits, then the three data bytes, then the length. */
static void rf_apply_midi(int midi_event_count) {
    for (int i = 0; i < midi_event_count; ++i) {
        unsigned long long packed;
        __builtin_memcpy(&packed, rf_midi + (unsigned)i * 8u, sizeof packed);
        unsigned status = (unsigned)((packed >> 32) & 0xffu);
        unsigned data1 = (unsigned)((packed >> 40) & 0xffu);
        unsigned data2 = (unsigned)((packed >> 48) & 0xffu);
        unsigned length = (unsigned)((packed >> 56) & 0xffu);
        /* Control change, controller 7: the channel volume slider. */
        if (length == 3u && (status & 0xf0u) == 0xb0u && data1 == 7u) {
            rf_gain = (double)data2 / 127.0;
        }
    }
}

/* A parameter event is sixteen bytes: frame, index, then the value as a
 * double. Sample-accurate automation is delivered with the block it belongs
 * to; a gain can honour the last one and stay correct. */
static void rf_apply_parameters(int parameter_event_count) {
    for (int i = 0; i < parameter_event_count; ++i) {
        unsigned index;
        double value;
        const unsigned char *record = rf_parameters + (unsigned)i * 16u;
        __builtin_memcpy(&index, record + 4, sizeof index);
        __builtin_memcpy(&value, record + 8, sizeof value);
        if (index == 0u) {
            rf_gain = value;
        }
    }
}

RF_EXPORT("rackforge_process")
int rf_process(int frames, int input_channels, int output_channels, int midi_event_count,
               int parameter_event_count) {
    /* Bound the channel counts before multiplying by them. The host validates
     * a block against the capacities above before it ever calls, so this
     * cannot happen from a real host -- but a reference is read and copied,
     * and `frames * channels` on an unbounded count is signed overflow, which
     * C leaves undefined. Reject first, arithmetic second. */
    if (frames <= 0 || frames > RF_MAX_FRAMES || input_channels < 0 ||
        input_channels > RF_MAX_CHANNELS || output_channels < 0 ||
        output_channels > RF_MAX_CHANNELS || midi_event_count < 0 ||
        midi_event_count > RF_MAX_MIDI_EVENTS || parameter_event_count < 0 ||
        parameter_event_count > RF_MAX_PARAMETER_EVENTS) {
        return RF_INVALID_ARGUMENT;
    }
    if (!rf_prepared) {
        return RF_INVALID_STATE;
    }

    rf_apply_midi(midi_event_count);
    rf_apply_parameters(parameter_event_count);

    const float gain = (float)rf_gain;
    const int samples = frames * output_channels;
    for (int i = 0; i < samples; ++i) {
        /* Fewer input channels than output is normal -- the host may hand a
         * mono source to a stereo instrument -- so read the input channel
         * that exists and leave the rest silent rather than reading past it. */
        const int frame = output_channels > 0 ? i / output_channels : 0;
        const int channel = output_channels > 0 ? i % output_channels : 0;
        float sample = 0.0f;
        if (input_channels > 0) {
            const int source = frame * input_channels +
                               (channel < input_channels ? channel : input_channels - 1);
            sample = rf_input[source];
        }
        rf_output[i] = sample * gain;
    }
    return RF_OK;
}
