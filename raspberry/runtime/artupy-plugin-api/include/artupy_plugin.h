#ifndef ARTUPY_PLUGIN_H
#define ARTUPY_PLUGIN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ARTUPY_ABI_VERSION_MAJOR 1u
#define ARTUPY_ABI_VERSION_MINOR 1u
#define ARTUPY_ABI_VERSION \
    ((ARTUPY_ABI_VERSION_MAJOR << 16u) | ARTUPY_ABI_VERSION_MINOR)

#define ARTUPY_STATUS_OK 0
#define ARTUPY_STATUS_INVALID_ARGUMENT -1
#define ARTUPY_STATUS_INVALID_STATE -2
#define ARTUPY_STATUS_UNKNOWN_PARAMETER -3
#define ARTUPY_STATUS_INTERNAL_ERROR -4

#define ARTUPY_LOG_TRACE 0u
#define ARTUPY_LOG_DEBUG 1u
#define ARTUPY_LOG_INFO 2u
#define ARTUPY_LOG_WARN 3u
#define ARTUPY_LOG_ERROR 4u

typedef void (*ArtupyHostLogFnV1)(
    void *context,
    uint32_t level,
    const uint8_t *text,
    size_t length
);
typedef size_t (*ArtupyHostGetResourcePathFnV1)(
    void *context,
    const uint8_t *resource_id,
    size_t resource_id_length,
    uint8_t *destination,
    size_t capacity
);

typedef struct ArtupyHostApiV1 {
    uint32_t struct_size;
    uint32_t api_version;
    void *context;
    ArtupyHostLogFnV1 log;
    ArtupyHostGetResourcePathFnV1 get_resource_path;
} ArtupyHostApiV1;

typedef struct ArtupyMidiEventV1 {
    uint32_t frame;
    uint8_t length;
    uint8_t data[3];
} ArtupyMidiEventV1;

typedef struct ArtupyParameterEventV1 {
    uint32_t frame;
    uint32_t parameter_index;
    double value;
} ArtupyParameterEventV1;

typedef struct ArtupyProcessBlockV1 {
    uint32_t struct_size;
    uint32_t frames;
    uint32_t input_channels;
    uint32_t output_channels;
    const float *input_interleaved;
    float *output_interleaved;
    const ArtupyMidiEventV1 *midi_events;
    uint32_t midi_event_count;
    const ArtupyParameterEventV1 *parameter_events;
    uint32_t parameter_event_count;
} ArtupyProcessBlockV1;

typedef size_t (*ArtupyWriteBytesFnV1)(uint8_t *destination, size_t capacity);
typedef void *(*ArtupyCreateFnV1)(const ArtupyHostApiV1 *host);
typedef void (*ArtupyDestroyFnV1)(void *instance);
typedef int32_t (*ArtupyActivateFnV1)(
    void *instance,
    double sample_rate,
    uint32_t maximum_frames,
    uint32_t input_channels,
    uint32_t output_channels
);
typedef int32_t (*ArtupyInstanceFnV1)(void *instance);
typedef int32_t (*ArtupySetParameterFnV1)(
    void *instance,
    uint32_t parameter_index,
    double value
);
typedef int32_t (*ArtupyGetParameterFnV1)(
    void *instance,
    uint32_t parameter_index,
    double *value
);
typedef size_t (*ArtupySaveStateFnV1)(
    void *instance,
    uint8_t *destination,
    size_t capacity
);
typedef int32_t (*ArtupyLoadStateFnV1)(
    void *instance,
    const uint8_t *source,
    size_t length
);
typedef int32_t (*ArtupyLoadPresetFnV1)(
    void *instance,
    const uint8_t *preset_id,
    size_t length
);
typedef int32_t (*ArtupyProcessFnV1)(
    void *instance,
    const ArtupyProcessBlockV1 *block
);

typedef struct ArtupyPluginApiV1 {
    uint32_t struct_size;
    uint32_t api_version;
    ArtupyWriteBytesFnV1 runtime_descriptor_json;
    ArtupyWriteBytesFnV1 parameter_schema_json;
    ArtupyWriteBytesFnV1 preset_catalog_json;
    ArtupyCreateFnV1 create;
    ArtupyDestroyFnV1 destroy;
    ArtupyActivateFnV1 activate;
    ArtupyInstanceFnV1 deactivate;
    ArtupyInstanceFnV1 reset;
    ArtupySetParameterFnV1 set_parameter;
    ArtupyGetParameterFnV1 get_parameter;
    ArtupySaveStateFnV1 save_state;
    ArtupyLoadStateFnV1 load_state;
    ArtupyLoadPresetFnV1 load_preset;
    ArtupyProcessFnV1 process;
} ArtupyPluginApiV1;

typedef const ArtupyPluginApiV1 *(*ArtupyPluginEntryFnV1)(void);

const ArtupyPluginApiV1 *artupy_plugin_entry_v1(void);

#ifdef __cplusplus
}
#endif

#endif
