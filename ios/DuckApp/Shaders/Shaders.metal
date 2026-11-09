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
    // xyz: the light's travel direction; w: the floor's checker flag.
    float4 light;
    // x: metalness, y: roughness, z: the scene exposure.
    float4 material;
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
    float3 n = normalize(in.normal);
    float3 v = normalize(-in.world);
    // The shader negates the stored vector, so `light` points the way the
    // light travels: from the camera towards the duck.
    float3 l = normalize(-uniforms.light.xyz);
    float3 h = normalize(l + v);

    float metalness = clamp(uniforms.material.x, 0.0, 1.0);
    float roughness = clamp(uniforms.material.y, 0.04, 1.0);
    float exposure = uniforms.material.z;

    // The official scene's groundplane: a blue-grey checker.
    float3 base = uniforms.color.rgb;
    if (uniforms.light.w > 0.5) {
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

    float3 color = diffuse_albedo * (0.18 + 0.82 * ndotl) * 1.15
                 + specular * ndotl
                 + ambient * diffuse_albedo * 0.55
                 + ambient * f0 * (0.15 + 0.85 * rim);

    // Soft contact shadow under the duck: the floor darkens toward the feet.
    if (uniforms.light.w > 0.5) {
        float radial = length(in.world.xy);
        float shadow = smoothstep(0.02, 0.55, radial);
        color *= mix(0.55, 1.0, shadow);
    }

    color *= exposure;
    // Reinhard tone map, then a gentle gamma, so highlights roll off instead
    // of clipping to white.
    color = color / (color + float3(1.0));
    color = pow(color, float3(1.0 / 2.2));
    return float4(color, uniforms.color.a);
}
