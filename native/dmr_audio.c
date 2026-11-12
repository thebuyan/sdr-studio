// SPDX-License-Identifier: GPL-2.0-only
#include <math.h>
#include <stdlib.h>
#include <mbelib.h>

struct voice_state {
    mbe_parms current, previous, enhanced;
    float gain;
};

void *sdr_dmr_voice_new(void)
{
    struct voice_state *state = calloc(1, sizeof(*state));
    if (state) {
        mbe_initMbeParms(&state->current, &state->previous, &state->enhanced);
        state->gain = 7.0f;
    }
    return state;
}

void sdr_dmr_voice_free(void *state)
{
    free(state);
}

void sdr_dmr_voice_decode(void *opaque, char frame[4][24], short samples[160])
{
    struct voice_state *state = opaque;
    char data[49], status[128];
    int errors, total_errors;
    float pcm[160] = {0};
    mbe_processAmbe3600x2450Framef(pcm, &errors, &total_errors, status,
                                   frame, data, &state->current, &state->previous,
                                   &state->enhanced, 3);

    // Keep mbelib's nominal gain for quiet frames and limit peaks before
    // converting to integers. Recover the gain gradually between frames.
    float peak = 0.0f;
    for (int i = 0; i < 160; i++) {
        if (!isfinite(pcm[i]))
            pcm[i] = 0.0f;
        peak = fmaxf(peak, fabsf(pcm[i]));
    }
    state->gain = fminf(7.0f, state->gain + 0.15f);
    if (peak * state->gain > 24000.0f)
        state->gain = 24000.0f / peak;
    for (int i = 0; i < 160; i++)
        samples[i] = (short)(pcm[i] * state->gain);
}
