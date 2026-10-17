//! GLSL ES 3.00 shaders for the atmosphere, planet surface and sky.

pub const BACKGROUND_VS: &str = r#"#version 300 es
precision highp float;

out vec2 v_ndc;

void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_ndc = p * 2.0 - 1.0;
    gl_Position = vec4(v_ndc, 1.0, 1.0);
}
"#;

pub const BACKGROUND_FS: &str = r#"#version 300 es
precision highp float;

in vec2 v_ndc;
out vec4 outColor;

uniform vec3 u_cam_right;
uniform vec3 u_cam_up;
uniform vec3 u_cam_forward;
uniform float u_tan_half_fov;
uniform float u_aspect;
uniform vec3 u_sun_dir;
uniform sampler2D u_sky;
uniform float u_time;

float hash13(vec3 p3) {
    p3 = fract(p3 * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

void main() {
    vec3 dir = normalize(
        u_cam_forward
        + v_ndc.x * u_aspect * u_tan_half_fov * u_cam_right
        + v_ndc.y * u_tan_half_fov * u_cam_up
    );

    // Equirectangular star map.
    float u = atan(dir.z, dir.x) * 0.159154943 + 0.5;
    float v = asin(clamp(dir.y, -1.0, 1.0)) * 0.318309886 + 0.5;
    vec3 raw = texture(u_sky, vec2(u, v)).rgb;
    raw = pow(raw, vec3(0.82));
    vec3 stars = raw * vec3(0.88, 0.95, 1.12) * 1.65;

    // A soft, baked-looking galactic band. This is cheap enough to run at full DPR.
    vec3 band_normal = normalize(vec3(0.34, 0.78, -0.52));
    float band = exp(-pow(dot(dir, band_normal) * 3.6, 2.0));
    float grain = 0.5 + 0.5 * sin(dir.x * 11.0 + dir.z * 7.0)
                      * sin(dir.y * 17.0 - dir.z * 5.0 + 0.8);
    grain = 0.75 + 0.25 * grain;
    vec3 nebula = vec3(0.030, 0.070, 0.115) * band * grain;
    nebula += vec3(0.070, 0.030, 0.100) * band * band * 0.55;

    // Sparse, subtle pin-prick stars layered over the photographed sky.
    vec3 cell = floor(dir * 620.0);
    float h = hash13(cell);
    float sparkle = smoothstep(0.9975, 1.0, h);
    vec3 star_col = mix(vec3(0.65, 0.8, 1.0), vec3(1.0, 0.82, 0.62), hash13(cell + 8.0));
    stars += star_col * sparkle * 0.75;

    vec3 color = vec3(0.0015, 0.0030, 0.0070) + stars + nebula;

    // Distance sun glow. The disc itself is allowed to sit just outside the frame.
    float sun = max(dot(dir, normalize(u_sun_dir)), 0.0);
    color += vec3(1.0, 0.62, 0.33) * pow(sun, 260.0) * 0.8;
    color += vec3(1.0, 0.38, 0.15) * pow(sun, 22.0) * 0.020;

    float vig = smoothstep(1.75, 0.22, length(v_ndc));
    color *= mix(0.55, 1.0, vig);

    color = pow(max(color, 0.0), vec3(1.0 / 2.2));
    outColor = vec4(color, 1.0);
}
"#;

pub const EARTH_VS: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in vec3 a_tangent;

uniform mat4 u_view_proj;
uniform mat4 u_model;

out vec3 v_world_pos;
out vec3 v_normal;
out vec3 v_tangent;
out vec2 v_uv;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    mat3 model3 = mat3(u_model);
    v_world_pos = world.xyz;
    v_normal = normalize(model3 * a_normal);
    v_tangent = normalize(model3 * a_tangent);
    v_uv = a_uv;
    gl_Position = u_view_proj * world;
}
"#;

pub const EARTH_FS: &str = r#"#version 300 es
precision highp float;

in vec3 v_world_pos;
in vec3 v_normal;
in vec3 v_tangent;
in vec2 v_uv;

uniform sampler2D u_day;
uniform sampler2D u_night;
uniform sampler2D u_water;
uniform sampler2D u_clouds;
uniform sampler2D u_normal;
uniform vec3 u_camera_pos;
uniform vec3 u_sun_dir;
uniform float u_cloud_offset;
uniform float u_cloud_opacity;
uniform float u_night_intensity;
uniform float u_normal_strength;

out vec4 outColor;

vec3 aces(vec3 x) {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

void main() {
    vec3 N = normalize(v_normal);
    vec3 T = normalize(v_tangent - N * dot(N, v_tangent));
    vec3 B = normalize(cross(N, T));

    vec3 nmap = texture(u_normal, v_uv).rgb * 2.0 - 1.0;
    nmap.xy *= u_normal_strength;
    N = normalize(T * nmap.x + B * nmap.y + N * max(nmap.z, 0.18));

    vec3 L = normalize(u_sun_dir);
    vec3 V = normalize(u_camera_pos - v_world_pos);
    float ndl = dot(N, L);
    float direct = max(ndl, 0.0);
    float day = smoothstep(-0.14, 0.22, ndl);
    float night = 1.0 - day;

    vec3 albedo = texture(u_day, v_uv).rgb;
    float water = texture(u_water, v_uv).r;
    water = smoothstep(0.22, 0.78, water);

    // The open ocean gets a deeper blue and a tight sun glint.
    vec3 land_color = albedo * (0.012 + 1.20 * direct);
    vec3 ocean_color = albedo * vec3(0.62, 0.82, 1.08) * (0.010 + 1.08 * direct);
    vec3 color = mix(land_color, ocean_color, water * 0.72);

    vec3 H = normalize(L + V);
    float glint = pow(max(dot(N, H), 0.0), 700.0);
    glint *= water * smoothstep(-0.03, 0.18, ndl) * 0.50;
    color += vec3(0.86, 0.94, 1.0) * glint;

    // Atmospheric scattering on the inside of the limb.
    float fresnel = pow(1.0 - max(dot(N, V), 0.0), 3.2);
    color += vec3(0.10, 0.40, 0.95) * fresnel * (0.10 + 0.82 * day);

    // Night side: a whisper of airglow plus the warm, dense city lights.
    vec3 raw_night = texture(u_night, v_uv).rgb;
    float night_lum = dot(raw_night, vec3(0.299, 0.587, 0.114));
    float city_mask = smoothstep(0.090, 0.36, night_lum);
    vec3 city = raw_night * city_mask * vec3(1.86, 1.22, 0.66) * 3.2;
    vec3 airglow = raw_night * vec3(0.18, 0.30, 0.54) * 0.18;
    vec3 city_light = (city + airglow) * night * u_night_intensity;

    // Clouds drift above the surface and pick up the warm terminator.
    vec2 cloud_uv = vec2(fract(v_uv.x + u_cloud_offset), v_uv.y);
    float cloud = texture(u_clouds, cloud_uv).a;
    cloud = smoothstep(0.025, 0.86, cloud) * u_cloud_opacity;

    vec2 shadow_uv = cloud_uv + vec2(0.0036, -0.0022);
    float shadow = texture(u_clouds, shadow_uv).a * u_cloud_opacity;
    color *= 1.0 - shadow * 0.16 * day;

    vec3 cloud_light = vec3(0.98, 0.99, 1.0) * (0.020 + 0.88 * direct);
    float warm_rim = exp(-pow((ndl - 0.02) / 0.13, 2.0)) * 0.22;
    cloud_light += vec3(1.0, 0.46, 0.20) * warm_rim;
    color = mix(color, cloud_light, cloud * 0.90);

    // Cities glow through thinner cloud cover, as they do from orbit.
    color += city_light * (0.55 + 0.45 * (1.0 - cloud * 0.8));

    float terminator = exp(-pow(ndl / 0.10, 2.0));
    color += vec3(1.0, 0.30, 0.06) * terminator * 0.028;

    color *= 1.10;
    color = aces(color);
    color = pow(color, vec3(1.0 / 2.2));
    outColor = vec4(color, 1.0);
}
"#;

pub const ATMOSPHERE_VS: &str = r#"#version 300 es
precision highp float;

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;

uniform mat4 u_view_proj;
uniform mat4 u_model;

out vec3 v_world_pos;
out vec3 v_normal;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world_pos = world.xyz;
    v_normal = normalize(mat3(u_model) * a_normal);
    gl_Position = u_view_proj * world;
}
"#;

pub const ATMOSPHERE_FS: &str = r#"#version 300 es
precision highp float;

in vec3 v_world_pos;
in vec3 v_normal;

uniform vec3 u_camera_pos;
uniform vec3 u_sun_dir;
uniform float u_strength;
uniform vec2 u_screen_center;
uniform float u_earth_radius_px;

out vec4 outColor;

void main() {
    vec3 N = normalize(v_normal);
    vec3 V = normalize(u_camera_pos - v_world_pos);
    vec3 L = normalize(u_sun_dir);

    float rim = pow(1.0 - abs(dot(N, V)), 2.4);
    float sun = smoothstep(-0.55, 0.72, dot(N, L));
    vec3 color = mix(vec3(0.055, 0.17, 0.50), vec3(0.29, 0.64, 1.0), sun);
    float alpha = rim * (0.065 + 0.78 * sun) * u_strength;
    alpha += rim * 0.030;

    // The shell is a hair larger than the planet. Fade it out at the shell's
    // silhouette so the halo has no visible geometric edge.
    float radial = length((gl_FragCoord.xy - u_screen_center) / max(u_earth_radius_px * 1.075, 1.0));
    alpha *= smoothstep(1.0, 0.92, radial);

    color = pow(max(color, 0.0), vec3(1.0 / 2.2)) * alpha;
    outColor = vec4(color, 1.0);
}
"#;
