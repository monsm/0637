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
};

struct Uniforms {
    float4x4 mvp;
    float4x4 model;
    float4 color;
    float4 light;
};

vertex VertexOut duck_vertex(
    const device VertexIn* vertices [[buffer(0)]],
    constant Uniforms& uniforms [[buffer(1)]],
    uint vertexID [[vertex_id]]
) {
    VertexIn input = vertices[vertexID];
    VertexOut out;
    out.position = uniforms.mvp * float4(input.px, input.py, input.pz, 1.0);
    out.normal = normalize((uniforms.model * float4(input.nx, input.ny, input.nz, 0.0)).xyz);
    return out;
}

fragment float4 duck_fragment(
    VertexOut in [[stage_in]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    float3 normal = normalize(in.normal);
    float3 light = normalize(-uniforms.light.xyz);
    float diffuse = max(dot(normal, light), 0.0);
    float rim = pow(1.0 - max(normal.z, 0.0), 2.0);
    float3 color = uniforms.color.rgb * (0.30 + 0.70 * diffuse) + rim * 0.06;
    return float4(color, uniforms.color.a);
}
