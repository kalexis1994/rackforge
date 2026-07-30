#ifndef RACKFORGE_PLUGIN_H
#define RACKFORGE_PLUGIN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RACKFORGE_ABI_VERSION_MAJOR 1u
#define RACKFORGE_ABI_VERSION_MINOR 4u
#define RACKFORGE_ABI_VERSION \
    ((RACKFORGE_ABI_VERSION_MAJOR << 16u) | RACKFORGE_ABI_VERSION_MINOR)

#define RACKFORGE_STATUS_OK 0
#define RACKFORGE_STATUS_INVALID_ARGUMENT -1
#define RACKFORGE_STATUS_INVALID_STATE -2
#define RACKFORGE_STATUS_UNKNOWN_PARAMETER -3
#define RACKFORGE_STATUS_INTERNAL_ERROR -4

#define RACKFORGE_LOG_TRACE 0u
#define RACKFORGE_LOG_DEBUG 1u
#define RACKFORGE_LOG_INFO 2u
#define RACKFORGE_LOG_WARN 3u
#define RACKFORGE_LOG_ERROR 4u

typedef void (*RackForgeHostLogFnV1)(
    void *context,
    uint32_t level,
    const uint8_t *text,
    size_t length
);
typedef size_t (*RackForgeHostGetResourcePathFnV1)(
    void *context,
    const uint8_t *resource_id,
    size_t resource_id_length,
    uint8_t *destination,
    size_t capacity
);
typedef size_t (*RackForgeHostGetPluginDataPathFnV1)(
    void *context,
    uint8_t *destination,
    size_t capacity
);
typedef int32_t (*RackForgeHostPublishPresetCatalogFnV1)(
    void *context,
    const uint8_t *source,
    size_t length
);

typedef struct RackForgeHostApiV1 {
    uint32_t struct_size;
    uint32_t api_version;
    void *context;
    RackForgeHostLogFnV1 log;
    RackForgeHostGetResourcePathFnV1 get_resource_path;
    RackForgeHostGetPluginDataPathFnV1 get_plugin_data_path;
    RackForgeHostPublishPresetCatalogFnV1 publish_preset_catalog;
} RackForgeHostApiV1;

typedef struct RackForgeMidiEventV1 {
    uint32_t frame;
    uint8_t length;
    uint8_t data[3];
} RackForgeMidiEventV1;

typedef struct RackForgeParameterEventV1 {
    uint32_t frame;
    uint32_t parameter_index;
    double value;
} RackForgeParameterEventV1;

typedef struct RackForgeProcessBlockV1 {
    uint32_t struct_size;
    uint32_t frames;
    uint32_t input_channels;
    uint32_t output_channels;
    const float *input_interleaved;
    float *output_interleaved;
    const RackForgeMidiEventV1 *midi_events;
    uint32_t midi_event_count;
    const RackForgeParameterEventV1 *parameter_events;
    uint32_t parameter_event_count;
} RackForgeProcessBlockV1;

typedef size_t (*RackForgeWriteBytesFnV1)(uint8_t *destination, size_t capacity);
typedef void *(*RackForgeCreateFnV1)(const RackForgeHostApiV1 *host);
typedef void (*RackForgeDestroyFnV1)(void *instance);
typedef int32_t (*RackForgeActivateFnV1)(
    void *instance,
    double sample_rate,
    uint32_t maximum_frames,
    uint32_t input_channels,
    uint32_t output_channels
);
typedef int32_t (*RackForgeInstanceFnV1)(void *instance);
typedef int32_t (*RackForgeSetParameterFnV1)(
    void *instance,
    uint32_t parameter_index,
    double value
);
typedef int32_t (*RackForgeGetParameterFnV1)(
    void *instance,
    uint32_t parameter_index,
    double *value
);
typedef size_t (*RackForgeSaveStateFnV1)(
    void *instance,
    uint8_t *destination,
    size_t capacity
);
typedef int32_t (*RackForgeLoadStateFnV1)(
    void *instance,
    const uint8_t *source,
    size_t length
);
typedef int32_t (*RackForgeLoadPresetFnV1)(
    void *instance,
    const uint8_t *preset_id,
    size_t length
);
typedef int32_t (*RackForgeProcessFnV1)(
    void *instance,
    const RackForgeProcessBlockV1 *block
);

typedef struct RackForgePluginApiV1 {
    uint32_t struct_size;
    uint32_t api_version;
    RackForgeWriteBytesFnV1 runtime_descriptor_json;
    RackForgeWriteBytesFnV1 parameter_schema_json;
    RackForgeWriteBytesFnV1 preset_catalog_json;
    RackForgeCreateFnV1 create;
    RackForgeDestroyFnV1 destroy;
    RackForgeActivateFnV1 activate;
    RackForgeInstanceFnV1 deactivate;
    RackForgeInstanceFnV1 reset;
    RackForgeSetParameterFnV1 set_parameter;
    RackForgeGetParameterFnV1 get_parameter;
    RackForgeSaveStateFnV1 save_state;
    RackForgeLoadStateFnV1 load_state;
    RackForgeLoadPresetFnV1 load_preset;
    RackForgeProcessFnV1 process;
} RackForgePluginApiV1;

typedef const RackForgePluginApiV1 *(*RackForgePluginEntryFnV1)(void);

const RackForgePluginApiV1 *rackforge_plugin_entry_v1(void);

#ifdef __cplusplus
}
#endif

#endif
