# Whisper's CPU backend must also be portable when a GPU backend is enabled.
# FORCE replaces instruction flags left in an existing CMake cache.
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
foreach(feature AVX AVX2 FMA F16C)
    set(GGML_${feature} ON CACHE BOOL "" FORCE)
endforeach()
foreach(feature AVX_VNNI AVX512 AVX512_VBMI AVX512_VNNI AVX512_BF16 AMX_TILE AMX_INT8 AMX_BF16)
    set(GGML_${feature} OFF CACHE BOOL "" FORCE)
endforeach()
# On arm64 use the compiler's target default, without host-specific extensions.
set(GGML_CPU_ARM_ARCH "" CACHE STRING "" FORCE)
