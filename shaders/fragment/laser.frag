#version 450

const float PI = 3.141592653589793;
const float attenuation = 0.9;

layout(set = 1, binding = 0) uniform LaserArgs {
    vec3 basis;
    mat4 transform;
};

layout(location = 0) in VertexData {
    vec3 position;
    vec2 tex_coord;
    vec4 color;
} vertex;

layout(location = 0) out vec4 color;

void main() {
    color = vertex.color;
    color.rgb *= pow(attenuation, distance(vertex.position, basis));
    float x_scan = vertex.tex_coord.x * 2. - 1.;
    color.rgb *= 1. / max(0.16, sqrt(1. - x_scan * x_scan)) / PI;
}
