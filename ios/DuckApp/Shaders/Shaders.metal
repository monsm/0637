#include <metal_stdlib>
using namespace metal;

struct VertexIn {
    float px;
    float py;
    float pz;
    float nx;
    float ny;
    float nz;
};

struct VertexOut {
    float4 position [[position]];
    float3 normal;
    float3 world;
};

struct Uniforms {
    float4x4 mvp;
    float4x4 model;
    float4 color;
    // xyz: the light's travel direction; w: the floor flag（1 = 地板，
    // 才有棋盘与接触阴影）。棋盘与否由下面 stage.x 再分一次。
    float4 light;
    // x: metalness, y: roughness, z: the scene exposure.
    float4 material;
    // —— Live 舞台的"房间"字段，只追加在尾部，与 Swift Uniforms 对齐 ——
    // x: 地板观感（0 = Live 暖石，1 = Instrument 冷棋盘）；y: 全屏背景板标志。
    float4 stage;
    // xy: 鸭子投到屏幕的 NDC（背景光晕中心）；z: 光晕半径；w: 屏幕宽高比。
    float4 halo;
    // rgb: 晨昏光色；w: 曝光系数。Swift 按真实本地时间算好，这里只做乘法。
    float4 daylight;
    // xy: 鸭子躯干的世界 xy（接触阴影中心）；z: 影子浓度（越大越实）；w 备用。
    float4 shadow;
};

// A cheap analytic environment: a sky gradient plus a ground bounce, so a
// dielectric has something to reflect and a metal has something to mirror.
// This is what turns the flat-shaded model into something that reads as a
// product rather than a screenshot of a simulator.
static float3 environment(float3 direction) {
    float up = clamp(direction.z * 0.5 + 0.5, 0.0, 1.0);
    float3 sky = mix(float3(0.16, 0.19, 0.24), float3(0.55, 0.65, 0.80), up);
    float3 ground = float3(0.10, 0.11, 0.13);
    float horizon = smoothstep(-0.15, 0.15, direction.z);
    return mix(ground, sky, horizon);
}

static float distribution_ggx(float3 n, float3 h, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float ndoth = max(dot(n, h), 0.0);
    float denom = ndoth * ndoth * (a2 - 1.0) + 1.0;
    return a2 / max(3.14159265 * denom * denom, 1e-5);
}

static float geometry_schlick(float ndotv, float roughness) {
    float k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    return ndotv / max(ndotv * (1.0 - k) + k, 1e-5);
}

static float3 fresnel_schlick(float cos_theta, float3 f0) {
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
}

vertex VertexOut duck_vertex(
    const device VertexIn* vertices [[buffer(0)]],
    constant Uniforms& uniforms [[buffer(1)]],
    uint vertexID [[vertex_id]]
) {
    VertexIn input = vertices[vertexID];
    VertexOut out;
    out.position = uniforms.mvp * float4(input.px, input.py, input.pz, 1.0);
    out.normal = normalize((uniforms.model * float4(input.nx, input.ny, input.nz, 0.0)).xyz);
    out.world = (uniforms.model * float4(input.px, input.py, input.pz, 1.0)).xyz;
    return out;
}

fragment float4 duck_fragment(
    VertexOut in [[stage_in]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    // 全屏背景板：不走 PBR，直接给出竖向渐变墙加鸭身后的暖光晕
    // （STAGE_LOOK §2.2）。渐变取 NDC 的 y 而不是世界高度——不管横竖屏、
    // 相机怎么动，地平线以上永远有一层从暖褐到冷蓝黑的墙。
    if (uniforms.stage.y > 0.5) {
        float2 ndc = in.position.xy / in.position.w;
        float t = ndc.y * 0.5 + 0.5;
        float3 horizon = float3(0.102, 0.090, 0.078);  // #1A1714 暖褐
        float3 zenith = float3(0.047, 0.055, 0.071);   // #0C0E12 冷蓝黑
        float3 bg = mix(horizon, zenith, smoothstep(0.62, 1.15, t));
        // 鸭身后的径向暖光 #FFD9A8 @ 8%：x 先乘宽高比，光晕才是圆的。
        float dx = (ndc.x - uniforms.halo.x) * uniforms.halo.w;
        float dy = ndc.y - uniforms.halo.y;
        float glow = exp(-(dx * dx + dy * dy)
            / max(uniforms.halo.z * uniforms.halo.z, 1e-5) * 2.2);
        bg += float3(1.0, 0.851, 0.659) * glow * 0.08;
        // 晨昏光与曝光：Swift 按真实本地时间算好，这里只做乘法。
        bg *= uniforms.daylight.rgb * uniforms.daylight.w;
        // 与主体同一套软膝压高光，背景和鸭子才像在同一台相机里。
        bg = clamp(bg, 0.0, 4.0);
        bg = mix(bg, bg / (bg + float3(0.55)) * 1.55,
            smoothstep(0.72, 1.6, max(max(bg.r, bg.g), bg.b)));
        return float4(clamp(bg, 0.0, 1.0), 1.0);
    }

    float3 n = normalize(in.normal);
    float3 v = normalize(-in.world);
    // The shader negates the stored vector, so `light` points the way the
    // light travels: from the camera towards the duck.
    float3 l = normalize(-uniforms.light.xyz);
    float3 h = normalize(l + v);

    float metalness = clamp(uniforms.material.x, 0.0, 1.0);
    float roughness = clamp(uniforms.material.y, 0.04, 1.0);
    float exposure = uniforms.material.z;

    // 地板颜色：Live 用 Swift 传来的暖石单色；Instrument 的冷棋盘
    // （STAGE_LOOK §3，0.2 m 一格）只在 stage.x 点名时画——网格是
    // 仪表盘的语言，Live 不再用它。
    float3 base = uniforms.color.rgb;
    if (uniforms.light.w > 0.5 && uniforms.stage.x > 0.5) {
        float2 cell = floor(in.world.xy / 0.2);
        float odd = fmod(cell.x + cell.y, 2.0);
        base = mix(float3(0.20, 0.30, 0.40), float3(0.10, 0.20, 0.30), odd);
    }

    float3 f0 = mix(float3(0.04), base, metalness);
    float3 diffuse_albedo = base * (1.0 - metalness);

    float ndotl = max(dot(n, l), 0.0);
    float ndotv = max(dot(n, v), 1e-4);

    // Cook-Torrance: specular from the key light.
    float d = distribution_ggx(n, h, roughness);
    float g = geometry_schlick(ndotv, roughness) * geometry_schlick(ndotl, roughness);
    float3 f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    float3 specular = (d * g) * f / max(4.0 * ndotv * ndotl, 1e-4);

    // A cheap ambient: sky above, bounce below, and a fresnel rim.
    float3 ambient_dir = reflect(-v, n);
    float3 ambient = environment(ambient_dir) * mix(0.35, 1.0, metalness);
    float rim = pow(1.0 - ndotv, 3.0);

    // The duck is a painted toy: most of what you see is diffuse. The albedo
    // term carries the material's colour, and the ambient lift keeps the
    // shadowed side from going black. 晨昏光的分工：房间吃满时辰色，鸭子是
    // 灯下的小物件，直射光保持中性——白壳按 display-space 出厂色走，环境
    // 只沾六成时辰色。否则夜里白壳会被染成青铜（CI 壳检查守的是
    // 「亮度 ≥120 的近中性像素」，那正是白壳该长成的样子）。白天 key 仍
    // 近似纯白，照明与原来一致。
    float isFloor = uniforms.light.w > 0.5 ? 1.0 : 0.0;
    float3 key = mix(float3(1.0), uniforms.daylight.rgb, isFloor * 0.6);
    float3 color = diffuse_albedo * (0.45 + 0.75 * ndotl) * key
                 + specular * ndotl * key
                 + ambient * diffuse_albedo * 0.85 * mix(float3(1.0), key, 0.5)
                 + ambient * f0 * (0.15 + 0.85 * rim);

    // 地板专属的三步：先保底、再压接触阴影、最后向地平线雾化。
    if (uniforms.light.w > 0.5) {
        // A ground plane seen from a shallow angle gets almost no key light,
        // so give it its own ambient floor or the room reads as a void.
        // 保底放在阴影之前，影子才能真正压得下去。
        color = max(color, base * 0.55);
        // 接触阴影跟着鸭子走：中心是 Swift 传来的躯干世界 xy，不再写死
        // 在家的位置。系数 0.62，影子更实，鸭脚踩得住（STAGE_LOOK §2.1）。
        float radial = length(in.world.xy - uniforms.shadow.xy);
        float shadow = smoothstep(0.02, 0.55, radial);
        color *= mix(1.0 - uniforms.shadow.z, 1.0, shadow);
        // 地平线处的极淡雾化：地板远端融进墙面，8 m 平面不再露出硬边。
        float fog = smoothstep(1.5, 4.0, in.world.y) * 0.35;
        color = mix(color,
            float3(0.102, 0.090, 0.078) * uniforms.daylight.rgb, fog);
    }

    // The scene's colours are authored in display space, so the albedo must
    // reach the screen unchanged; only highlights get a soft knee. A Reinhard
    // curve over the whole surface washed the duck's orange out to beige.
    // 曝光：房间吃满夜压（夜里整体低一档，任务 §4），鸭子本体只压两成——
    // 暗的是屋，不是灯下那只白壳的鸭。
    color *= exposure * mix(1.0, uniforms.daylight.w, isFloor * 0.8 + 0.2);
    color = clamp(color, 0.0, 4.0);
    color = mix(color, color / (color + float3(0.55)) * 1.55, smoothstep(0.72, 1.6, max(max(color.r, color.g), color.b)));
    color = clamp(color, 0.0, 1.0);
    return float4(color, uniforms.color.a);
}
