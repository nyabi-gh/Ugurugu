find_package(Qt6 6.11 REQUIRED COMPONENTS Core Gui)

qt_standard_project_setup()

add_library(ugurugu_engine STATIC ${UGURUGU_ENGINE_SOURCES})
target_include_directories(ugurugu_engine PUBLIC src)
target_link_libraries(
    ugurugu_engine
    PUBLIC
    Qt6::Core
    Qt6::Gui
)
ugurugu_target_defaults(ugurugu_engine)
# LTO covers only our own objects; the prebuilt Qt static archives stay
# regular object files and wasm-ld links the mix as-is.
target_compile_options(ugurugu_engine PRIVATE -flto)

# Plain add_executable on purpose: the engine runs headless, so it must not
# pull in the wasm QPA plugin or the qtloader HTML shell that
# qt_add_executable's finalization generates.
add_executable(
    ugurugu_engine_spike
    src/wasm/BridgeDocument.cpp
    src/wasm/BridgeDocument.hpp
    src/wasm/EngineBridge.cpp
    src/wasm/EngineBridgeContent.cpp
    src/wasm/EngineBridgeClipboard.cpp
    src/wasm/EngineBridgeExport.cpp
    src/wasm/EngineBridgeLayers.cpp
    src/wasm/EngineBridgeRender.cpp
    src/wasm/EngineBridgeSelection.cpp
    src/wasm/EngineBridgeStrokes.cpp
)
target_link_libraries(ugurugu_engine_spike PRIVATE ugurugu_engine)
ugurugu_target_defaults(ugurugu_engine_spike)
target_compile_options(ugurugu_engine_spike PRIVATE -flto)
# The headless engine never shows a window or decodes gif/ico/jpeg payloads,
# so the wasm QPA plugin (and the Qt6OpenGL it drags in) and the image format
# plugins stay out of the binary.
qt_import_plugins(
    ugurugu_engine_spike
    EXCLUDE_BY_TYPE
    platforms
    imageformats
)
target_link_options(
    ugurugu_engine_spike
    PRIVATE
    # CMake's Release link line carries no -O flag, which emcc treats as -O0
    # and skips wasm-opt plus JS minification entirely; -O3 must be stated
    # here explicitly.
    -O3
    -flto
    --no-entry
    # Qt Core and Gui use emscripten::val internally, which needs embind's
    # JS-side runtime even in a headless engine.
    -lembind
    -sMODULARIZE=1
    -sEXPORT_NAME=createUguruguEngine
    -sENVIRONMENT=web,worker,node
    -sALLOW_MEMORY_GROWTH=1
    # A named allocation failure the shell can report, rather than the
    # browser's own 2 GiB limit killing the tab. The worst measured document
    # peaks at 150 MiB.
    -sMAXIMUM_MEMORY=512MB
    -sEXPORTED_FUNCTIONS=_malloc,_free
    -sEXPORTED_RUNTIME_METHODS=ccall,cwrap,HEAPU8,UTF8ToString
)
