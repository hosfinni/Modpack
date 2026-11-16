override sky_glow: bool = true;

// The sky along one ray. Vanilla's colour, with a warm glow round the sun while it
// is low and the season's mood: a touch warmer in summer, paler in winter.
fn sky(input: SkyInput) -> vec4<f32> {
    var color = input.color.rgb;
    if sky_glow && frame.dimension == 0 {
        let toward_sun = max(dot(input.direction, frame.sun_direction), 0.0);
        // Only near sunrise and sunset, and only near the horizon.
        let low = 1.0 - smoothstep(0.0, 0.45, abs(frame.sun_direction.y));
        let horizon = 1.0 - smoothstep(0.0, 0.5, abs(input.direction.y));
        let glow = pow(toward_sun, 6.0) * low * horizon * (1.0 - frame.rain);
        color += vec3<f32>(1.0, 0.55, 0.25) * glow * 0.35;

        let warmth = season_warmth();
        color *= mix(vec3<f32>(1.0), vec3<f32>(1.04, 1.0, 0.94), max(warmth, 0.0) * 0.5);
        let pale = vec3<f32>(luma(color)) * vec3<f32>(0.97, 1.0, 1.05);
        color = mix(color, pale, max(-warmth, 0.0) * 0.18);
    }
    return vec4<f32>(color, input.color.a);
}
