override shafts: bool = true;
override shaft_strength: f32 = 0.35;
override vignette: bool = true;

// How many times the picture is read along the way to the sun. Each is one depth
// read, and they are the whole cost of the pass.
const SHAFT_TAPS: i32 = 10;

// Where the body lighting the world (the sun, or the moon at night) is on the
// screen: xy from the top left, z above 0 while it is in front of the camera.
fn light_on_screen() -> vec3<f32> {
    let clip = frame.projection * frame.view * vec4<f32>(frame.light_direction * 100.0, 1.0);
    if clip.w <= 0.0 {
        return vec3<f32>(0.0, 0.0, -1.0);
    }
    let ndc = clip.xy / clip.w;
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, 1.0);
}

// How much open sky lies between this pixel and the light, 0 to 1: the picture's
// depth is read a few times along the way, and the sky is where nothing was drawn.
fn sky_toward(uv: vec2<f32>, pixel: vec2<f32>, light: vec2<f32>) -> f32 {
    let stride = (light - uv) / f32(SHAFT_TAPS) * 0.9;
    // Each pixel starts a little further along or back, which turns the bands the
    // few reads would leave into fine grain.
    let grain = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
    var at = uv + stride * grain;
    var open = 0.0;
    var total = 0.0;
    for (var i = 0; i < SHAFT_TAPS; i++) {
        at += stride;
        // Reads near the pixel count for more than reads near the light.
        let weight = 1.0 - f32(i) / f32(SHAFT_TAPS);
        let inside = all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0));
        if inside && depth_at(at) == 0.0 {
            open += weight;
        }
        total += weight;
    }
    return open / total;
}

// One pixel of the finished picture: shafts of light from the sun or the moon, then
// a mild grade, the season's mood and a vignette.
fn post(input: PostInput) -> vec4<f32> {
    var color = input.color.rgb;

    // In the Overworld, with the camera in the air.
    if shafts && frame.dimension == 0 && frame.eye_in == 0u {
        let light = light_on_screen();
        let height = frame.light_direction.y;
        if light.z > 0.0 && height > -0.05 {
            let aspect = vec2<f32>(frame.resolution.x / frame.resolution.y, 1.0);
            let away = length((light.xy - input.uv) * aspect);
            let near = 1.0 - smoothstep(0.0, 0.9, away);
            if near > 0.0 {
                let open = sky_toward(input.uv, input.pixel, light.xy);
                // Golden by day, a faint blue by night; stronger while the light is low.
                let glow = mix(vec3<f32>(0.14, 0.17, 0.24), vec3<f32>(1.0, 0.82, 0.55), daylight());
                let low = 1.0 + (1.0 - smoothstep(0.1, 0.6, height)) * 0.8;
                let clear = (1.0 - frame.rain * 0.8) * (1.0 - frame.blindness);
                color += glow * open * near * near * shaft_strength * low * clear;
            }
        }
    }

    // A little more contrast about mid grey and a little more colour.
    let grey = luma(color);
    color = mix(vec3<f32>(grey), color, 1.05);
    color = max((color - vec3<f32>(0.18)) * 1.05 + vec3<f32>(0.18), vec3<f32>(0.0));

    // The season: a warm cast in summer, a cool pale one in winter.
    let warmth = season_warmth();
    color *= mix(vec3<f32>(1.0), vec3<f32>(1.03, 1.0, 0.95), max(warmth, 0.0));
    let winter = vec3<f32>(luma(color)) * vec3<f32>(0.96, 1.0, 1.05);
    color = mix(color, winter, max(-warmth, 0.0) * 0.14);

    if vignette {
        let from_centre = distance(input.uv, vec2<f32>(0.5));
        color *= 1.0 - smoothstep(0.45, 0.95, from_centre) * 0.28;
    }
    return vec4<f32>(color, input.color.a);
}
