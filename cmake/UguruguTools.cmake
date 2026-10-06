set(
    UGURUGU_LLVM_TOOL_HINTS
    "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin"
    "/opt/homebrew/opt/llvm/bin"
    "/usr/local/opt/llvm/bin"
)
if(WIN32 AND CMAKE_GENERATOR_INSTANCE)
    list(
        APPEND
        UGURUGU_LLVM_TOOL_HINTS
        "${CMAKE_GENERATOR_INSTANCE}/VC/Tools/Llvm/x64/bin"
        "${CMAKE_GENERATOR_INSTANCE}/VC/Tools/Llvm/bin"
    )
endif()

find_program(
    UGURUGU_CLANG_FORMAT
    NAMES clang-format
    HINTS ${UGURUGU_LLVM_TOOL_HINTS}
)
if(UGURUGU_CLANG_FORMAT)
    file(
        GLOB_RECURSE
        UGURUGU_FORMAT_SOURCES
        CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.mm"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.hpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.hpp"
    )
    add_custom_target(
        ugurugu_format
        COMMAND
        "${UGURUGU_CLANG_FORMAT}"
        -i
        ${UGURUGU_FORMAT_SOURCES}
        VERBATIM
    )
    add_custom_target(
        ugurugu_format_check
        COMMAND
        "${UGURUGU_CLANG_FORMAT}"
        --dry-run
        --Werror
        ${UGURUGU_FORMAT_SOURCES}
        VERBATIM
    )
endif()

find_program(
    UGURUGU_CLANG_TIDY
    NAMES clang-tidy
    HINTS ${UGURUGU_LLVM_TOOL_HINTS}
)
find_program(
    UGURUGU_RUN_CLANG_TIDY
    NAMES run-clang-tidy run-clang-tidy.py
    HINTS ${UGURUGU_LLVM_TOOL_HINTS}
)
set(
    UGURUGU_RUN_CLANG_TIDY_COMMAND
    "${UGURUGU_RUN_CLANG_TIDY}"
)
if(WIN32 AND UGURUGU_RUN_CLANG_TIDY)
    find_package(Python3 QUIET COMPONENTS Interpreter)
    if(Python3_Interpreter_FOUND)
        list(
            PREPEND
            UGURUGU_RUN_CLANG_TIDY_COMMAND
            "${Python3_EXECUTABLE}"
        )
    else()
        set(UGURUGU_RUN_CLANG_TIDY_COMMAND)
    endif()
endif()
if(UGURUGU_CLANG_TIDY AND UGURUGU_RUN_CLANG_TIDY_COMMAND)
    set(UGURUGU_TIDY_PATH_SEPARATOR "[/\\\\]")
    string(
        REPLACE
        "/"
        "${UGURUGU_TIDY_PATH_SEPARATOR}"
        UGURUGU_TIDY_SOURCE_ROOT
        "${CMAKE_CURRENT_SOURCE_DIR}"
    )
    set(
        UGURUGU_TIDY_SOURCE_PATTERN
        "^${UGURUGU_TIDY_SOURCE_ROOT}${UGURUGU_TIDY_PATH_SEPARATOR}(src|tests|tools)${UGURUGU_TIDY_PATH_SEPARATOR}.*\\.(cpp|mm)$"
    )
    set(UGURUGU_CLANG_TIDY_ARGUMENTS)
    if(APPLE)
        execute_process(
            COMMAND xcrun --sdk macosx --show-sdk-path
            OUTPUT_VARIABLE UGURUGU_MACOS_SDK_PATH
            OUTPUT_STRIP_TRAILING_WHITESPACE
            RESULT_VARIABLE UGURUGU_MACOS_SDK_RESULT
        )
        if(UGURUGU_MACOS_SDK_RESULT EQUAL 0)
            list(
                APPEND
                UGURUGU_CLANG_TIDY_ARGUMENTS
                -extra-arg=-isysroot
                "-extra-arg=${UGURUGU_MACOS_SDK_PATH}"
            )
        endif()
    endif()
    add_custom_target(
        ugurugu_tidy
        COMMAND
        ${UGURUGU_RUN_CLANG_TIDY_COMMAND}
        -p
        "${CMAKE_BINARY_DIR}"
        -clang-tidy-binary
        "${UGURUGU_CLANG_TIDY}"
        -quiet
        ${UGURUGU_CLANG_TIDY_ARGUMENTS}
        "${UGURUGU_TIDY_SOURCE_PATTERN}"
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        USES_TERMINAL
        VERBATIM
    )
    add_dependencies(ugurugu_tidy Ugurugu)
    if(TARGET ugurugu_tests)
        add_dependencies(ugurugu_tidy ugurugu_tests)
    endif()
endif()

add_executable(
    ugurugu_raster_asset_probe
    EXCLUDE_FROM_ALL
    tools/RasterAssetBudgetProbe.cpp
)
target_link_libraries(
    ugurugu_raster_asset_probe
    PRIVATE
    Qt6::Core
    Qt6::Gui
)
ugurugu_target_defaults(ugurugu_raster_asset_probe)

add_executable(
    ugurugu_fill_representation_probe
    EXCLUDE_FROM_ALL
    tools/FillRepresentationProbe.cpp
)
target_link_libraries(
    ugurugu_fill_representation_probe
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_fill_representation_probe)

add_executable(
    ugurugu_wawa_v10_probe
    EXCLUDE_FROM_ALL
    tools/WawaV10Probe.cpp
)
target_link_libraries(
    ugurugu_wawa_v10_probe
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_wawa_v10_probe)

add_executable(
    ugurugu_stress_document_generator
    EXCLUDE_FROM_ALL
    tools/StressDocumentGenerator.cpp
)
target_link_libraries(
    ugurugu_stress_document_generator
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_stress_document_generator)

add_executable(
    ugurugu_fixture_generator
    EXCLUDE_FROM_ALL
    tools/FixtureGenerator.cpp
)
target_link_libraries(
    ugurugu_fixture_generator
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_fixture_generator)

add_executable(
    ugurugu_engine_digest_probe
    EXCLUDE_FROM_ALL
    tools/EngineDigestProbe.cpp
)
target_link_libraries(
    ugurugu_engine_digest_probe
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_engine_digest_probe)

add_executable(
    ugurugu_render_benchmark
    EXCLUDE_FROM_ALL
    tools/RenderBenchmark.cpp
)
target_link_libraries(
    ugurugu_render_benchmark
    PRIVATE
    ugurugu_core
)
ugurugu_target_defaults(ugurugu_render_benchmark)
