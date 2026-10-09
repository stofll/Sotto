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

# With the default Visual Studio generator, cmake-rs replaces the per-configuration
# flags with cc's and strips every /O option, so MSVC would build ggml unoptimised
# (about ten times slower). Restore CMake's optimisation flags for that case.
if(CMAKE_GENERATOR MATCHES "^Visual Studio")
    foreach(lang C CXX)
        foreach(config RELEASE RELWITHDEBINFO)
            set(flags "${CMAKE_${lang}_FLAGS_${config}}")
            if(DEFINED CMAKE_${lang}_FLAGS_${config} AND NOT flags MATCHES "[-/]O[12x]")
                set(CMAKE_${lang}_FLAGS_${config} "${flags} /O2 /Ob2 /DNDEBUG" CACHE STRING "" FORCE)
            endif()
        endforeach()
    endforeach()
endif()
