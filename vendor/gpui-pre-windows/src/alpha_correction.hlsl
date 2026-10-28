// Adapted from https://github.com/microsoft/terminal/blob/1283c0f5b99a2961673249fa77c6b986efb5086c/src/renderer/atlas/dwrite.hlsl
// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

float color_brightness(float3 color) {
    // REC. 601 luminance coefficients for perceived brightness
    return dot(color, float3(0.30f, 0.59f, 0.11f));
}

// Argus patch: light text keeps the full enhanced contrast, as in Chromium.
// Upstream fades it out above 75% brightness, which draws white and light grey
// text on dark backgrounds visibly thinner than the same font in a browser.
float light_on_dark_contrast(float enhancedContrast, float3 color) {
    return enhancedContrast;
}

// Argus patch: dark text follows Skia's mask gamma, as Chromium draws it on
// Windows (gamma 1.2, contrast 0.2; the text luminance quantised to 3 bits and
// the background assumed to be its perceptual inverse). The DirectWrite
// correction above drew dark text on light backgrounds measurably heavier.
#define SKIA_GAMMA 1.2f
#define SKIA_CONTRAST 0.2f
float skia_luminance(float3 color) {
    float3 lin = pow(saturate(color), SKIA_GAMMA);
    return pow(dot(lin, float3(0.2126f, 0.7152f, 0.0722f)), 1.0f / SKIA_GAMMA);
}
float skia_mask_gamma(float coverage, float3 color) {
    float level = min(floor(skia_luminance(color) * 8.0f), 7.0f);
    float src = level / 7.0f;
    float dst = 1.0f - src;
    float lin_src = pow(src, SKIA_GAMMA);
    float lin_dst = pow(dst, SKIA_GAMMA);
    float srca = coverage + (1.0f - coverage) * SKIA_CONTRAST * lin_dst * coverage;
    if (abs(src - dst) < 1.0f / 256.0f) {
        return srca;
    }
    float out_value = pow(lin_src * srca + (1.0f - srca) * lin_dst, 1.0f / SKIA_GAMMA);
    return saturate((out_value - dst) / (src - dst));
}

float enhance_contrast(float alpha, float k) {
    return alpha * (k + 1.0f) / (alpha * k + 1.0f);
}

float3 enhance_contrast3(float3 alpha, float k) {
    return alpha * (k + 1.0f) / (alpha * k + 1.0f);
}

float apply_alpha_correction(float a, float b, float4 g) {
    float brightness_adjustment = g.x * b + g.y;
    float correction = brightness_adjustment * a + (g.z * b + g.w);
    return a + a * (1.0f - a) * correction;
}

float3 apply_alpha_correction3(float3 a, float3 b, float4 g) {
    float3 brightness_adjustment = g.x * b + g.y;
    float3 correction = brightness_adjustment * a + (g.z * b + g.w);
    return a + a * (1.0f - a) * correction;
}

float apply_contrast_and_gamma_correction(float sample, float3 color, float enhanced_contrast_factor, float4 gamma_ratios) {
    if (color_brightness(color) < 0.5f) {
        return skia_mask_gamma(sample, color);
    }
    float enhanced_contrast = light_on_dark_contrast(enhanced_contrast_factor, color);
    float brightness = color_brightness(color);

    float contrasted = enhance_contrast(sample, enhanced_contrast);
    return apply_alpha_correction(contrasted, brightness, gamma_ratios);
}

float3 apply_contrast_and_gamma_correction3(float3 sample, float3 color, float enhanced_contrast_factor, float4 gamma_ratios) {
    float enhanced_contrast = light_on_dark_contrast(enhanced_contrast_factor, color);

    float3 contrasted = enhance_contrast3(sample, enhanced_contrast);
    return apply_alpha_correction3(contrasted, color, gamma_ratios);
}
