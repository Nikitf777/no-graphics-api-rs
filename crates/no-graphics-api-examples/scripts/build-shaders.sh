#!/usr/bin/env bash
# Compile the example Slang shaders to SPIR-V with the same flags upstream's
# examples/NoGraphicsAPI_slang.cmake uses. Generated .spv files are committed, so building the
# examples does not require Slang; run this script only after editing a .slang file.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
vendor="$root/../no-graphics-api-sys/vendor/NoGraphicsAPI"
out="$root/shaders"
slangc=${SLANGC:-slangc}
spirv_val=${SPIRV_VAL:-spirv-val}

common=(
    -target spirv -profile spirv_1_5 -emit-spirv-directly
    -fvk-use-c-layout -matrix-layout-row-major
    -I "$out"
    -I "$vendor/include"
    -I "$vendor/utility/include"
    -capability spvDescriptorHeapEXT
)

compile() {
    local source=$1 entry=$2 stage=$3 output=$4
    shift 4
    "$slangc" "$out/$source" -fvk-use-entrypoint-name -entry "$entry" -stage "$stage" \
        "${common[@]}" "$@" -o "$out/$output"
    "$spirv_val" --target-env vulkan1.4 --scalar-block-layout "$out/$output"
    echo "compiled $source:$entry -> $output"
}

compile triangle.slang vertexMain vertex triangle.vert.spv
compile triangle.slang fragmentMain fragment triangle.frag.spv
compile cube.slang vertexMain vertex cube.vert.spv
compile cube.slang fragmentMain fragment cube.frag.spv
compile simulation.slang computeMain compute simulation.comp.spv
compile gbuffer.slang meshMain mesh gbuffer.mesh.spv -capability spvMeshShadingEXT
compile gbuffer.slang fragmentMain fragment gbuffer.frag.spv
compile deferred_lighting.slang vertexMain vertex deferred.vert.spv
compile deferred_lighting.slang fragmentMain fragment deferred.frag.spv
