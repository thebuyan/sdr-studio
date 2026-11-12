// SPDX-License-Identifier: GPL-2.0-only

#ifndef SDR_PLUGIN_H
#define SDR_PLUGIN_H
#include <stddef.h>
#include <stdint.h>
/* v1: hard dibits 0..3, 4800 symbols/s, valid only during process().
 * ABI for local trusted code. Each instance runs on one receiver thread.
 * No ownership of host memory; callbacks must not block or throw/panic.
 * shutdown() runs before dlclose(). No host callbacks in v1.
 */
typedef struct {
    uint32_t abi_version;
    const char *name;
    void (*process)(const uint8_t *dibits, size_t count);
    void (*shutdown)(void);
} SdrPluginV1;
SdrPluginV1 sdr_plugin_v1(void);
#endif
