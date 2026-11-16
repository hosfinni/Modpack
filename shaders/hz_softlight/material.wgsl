override soft_light: bool = true;
override light_warmth: f32 = 0.5;
override wet_ground: bool = true;
override shadows: bool = true;
override shadow_strength: f32 = 0.35;

// How much of the sun (or moon) reaches this point, 0..1: the shadow map, read at
// five points a texel and a half apart and averaged, so edges are soft. Faces turned
// from the light are left to the face shading below, and the shadow fades out toward
// the edge of the map, so it never ends in a line.
fn sunlit(input: MaterialInput) -> f32 {
    if !shadows || frame.has_shadow == 0u {
        return 1.0;
    }
    let facing = dot(input.normal, frame.light_direction);
    if facing <= 0.0 {
        return 1.0;
    }
    // Off the surface a little, more on faces the light grazes: no shadow acne.
    let bias = frame.shadow_center_texel * (1.5 + 2.0 * (1.0 - facing));
    let p = shadow_position(input.position + input.normal * bias);
    if any(p.xy <= vec2<f32>(0.0)) || any(p.xy >= vec2<f32>(1.0)) || p.z >= 1.0 {
        return 1.0;
    }
    let step = 1.5 / max(frame.shadow_resolution, 1.0);
    var lit = textureSampleCompareLevel(shadow_map, shadow_sampler, p.xy, p.z);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, p.xy + vec2<f32>(step, 0.0), p.z);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, p.xy - vec2<f32>(step, 0.0), p.z);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, p.xy + vec2<f32>(0.0, step), p.z);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, p.xy - vec2<f32>(0.0, step), p.z);
    lit *= 0.2;
    let far = length(input.position) / max(frame.shadow_distance, 1.0);
    return mix(lit, 1.0, smoothstep(0.7, 0.95, far));
}

// A colour with its brightness taken out: only its hue is left to tint with.
fn hue_of(c: vec3<f32>) -> vec3<f32> {
    return c / max(luma(c), 0.05);
}

// One point of a block's surface. Vanilla's is `albedo * tint * lightmap`; this
// keeps that and shapes the light a little:
// - block light is warm and skylight cool, by which of the two lights the point;
// - a coloured source (soul fire, lava, redstone) colours its own light;
// - the side of things facing the sun or the moon is brighter, the far side darker;
// - a block that shines glows a little over its own light;
// - open ground darkens in the rain.
// - with `shadows`, what stands between a point and the sun shades it, softly.
// Apart from those five shadow-map reads it costs a few multiplications a pixel.
fn material(input: MaterialInput) -> Surface {
    let base = input.albedo * input.tint;
    var light = input.lightmap;
    if soft_light {
        let block = input.light.x;
        let sky = input.light.y * daylight();
        // How much of the light here comes from blocks rather than the sky.
        let from_blocks = clamp(block / max(block + sky, 0.001), 0.0, 1.0);
        let warm = vec3<f32>(1.0, 0.86, 0.68);
        let cool = vec3<f32>(0.9, 0.96, 1.06);
        light *= mix(vec3<f32>(1.0), mix(cool, warm, from_blocks), light_warmth);

        let source = block_light_color(input.position);
        light *= mix(vec3<f32>(1.0), hue_of(source.rgb), clamp(source.a * from_blocks, 0.0, 1.0) * 0.6);

        // Only where the sky reaches, and less at dusk and dawn, when the light is
        // low and comes from nowhere in particular.
        let open = input.light.y * input.light.y;
        let strength = 0.14 * open * smoothstep(0.0, 0.3, abs(frame.light_direction.y));
        light *= 1.0 + strength * dot(input.normal, frame.light_direction);

        light += input.colored_light * 0.6;
    }
    if shadows {
        // Subtle: the shadowed part of the light is the sky's share of it, and less
        // when the sun is low. Torch light and caves are left alone.
        let open = input.light.y * input.light.y * smoothstep(0.02, 0.25, frame.light_direction.y);
        let shade = (1.0 - sunlit(input)) * shadow_strength * open * (0.4 + 0.6 * daylight());
        light *= 1.0 - shade;
    }
    var color = base.rgb * light;
    if soft_light {
        color += base.rgb * input.emission * 0.25;
    }
    if wet_ground && frame.precipitation == 1u {
        let facing_up = max(input.normal.y, 0.0);
        let open = smoothstep(0.8, 1.0, input.light.y);
        color *= 1.0 - 0.16 * frame.wetness * facing_up * open;
    }
    return Surface(vec4<f32>(color, base.a));
}
