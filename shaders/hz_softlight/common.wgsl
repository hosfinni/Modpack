// Shared by every stage of the pack.

// A colour's brightness, in the frame's primaries.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, frame.luminance);
}

// How much daylight there is: 0 at night, 1 once the sun is up.
fn daylight() -> f32 {
    return smoothstep(-0.08, 0.22, frame.sun_direction.y);
}

// The season, from the hz_seasons mod when it is installed: 1 at midsummer, -1 at
// midwinter. 0 without the mod, which leaves every stage as it is.
fn season_warmth() -> f32 {
    return clamp(frame.mods[0].y, -1.0, 1.0);
}
