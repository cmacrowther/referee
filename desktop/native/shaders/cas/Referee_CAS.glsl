// Derived from the AMD FidelityFX Contrast Adaptive Sharpening (CAS)
// sharpen-only path and adapted for libplacebo/mpv hook syntax.
// Original algorithm copyright (c) Advanced Micro Devices, Inc.
// Licensed under the MIT license.

//!DESC Referee-CAS-Sharpen
//!HOOK MAIN
//!BIND MAIN

#define REFEREE_CAS_STRENGTH {{CAS_STRENGTH}}

float referee_cas_peak() {
    return -1.0 / mix(8.0, 5.0, clamp(REFEREE_CAS_STRENGTH, 0.0, 1.0));
}

vec3 referee_cas_sample(vec2 offset) {
    return MAIN_texOff(offset).rgb;
}

vec4 hook() {
    vec4 center = MAIN_texOff(vec2(0.0, 0.0));

    vec3 a = referee_cas_sample(vec2(-1.0, -1.0));
    vec3 b = referee_cas_sample(vec2(0.0, -1.0));
    vec3 c = referee_cas_sample(vec2(1.0, -1.0));
    vec3 d = referee_cas_sample(vec2(-1.0, 0.0));
    vec3 e = center.rgb;
    vec3 f = referee_cas_sample(vec2(1.0, 0.0));
    vec3 g = referee_cas_sample(vec2(-1.0, 1.0));
    vec3 h = referee_cas_sample(vec2(0.0, 1.0));
    vec3 i = referee_cas_sample(vec2(1.0, 1.0));

    vec3 mn = min(min(min(d, e), f), min(b, h));
    vec3 mn2 = min(min(min(mn, a), c), min(g, i));
    mn += mn2;

    vec3 mx = max(max(max(d, e), f), max(b, h));
    vec3 mx2 = max(max(max(mx, a), c), max(g, i));
    mx += mx2;

    vec3 amp = sqrt(clamp(min(mn, vec3(2.0) - mx) / max(mx, vec3(1e-6)), 0.0, 1.0));
    vec3 weight = amp * vec3(referee_cas_peak());
    vec3 reciprocal = 1.0 / (1.0 + 4.0 * weight);
    vec3 sharpened = clamp((b * weight + d * weight + f * weight + h * weight + e) * reciprocal, 0.0, 1.0);

    return vec4(sharpened, center.a);
}
