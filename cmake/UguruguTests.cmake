if(NOT BUILD_TESTING)
    return()
endif()

find_package(Qt6 6.10 REQUIRED COMPONENTS Test)

qt_add_executable(ugurugu_tests ${UGURUGU_TEST_SOURCES})
target_include_directories(
    ugurugu_tests
    PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/tests"
)
target_link_libraries(
    ugurugu_tests
    PRIVATE
    ugurugu_ui
    Qt6::Test
    webpdemux
)
target_compile_definitions(
    ugurugu_tests
    PRIVATE
    UGURUGU_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    UGURUGU_VERSION="${PROJECT_VERSION}"
)
ugurugu_target_defaults(ugurugu_tests)

set(UGURUGU_TEST_SUITES
    app
    document
    render
    gif
    webp
    mask
    release_notes
    stabilizer
    ui_shell
    ui_selection
    ui_viewport
    ui_drawing_tools
    ui_session
)
foreach(suite IN LISTS UGURUGU_TEST_SUITES)
    set(test_name "ugurugu_${suite}_tests")
    set(test_timeout 180)
    if(suite STREQUAL "render")
        set(test_timeout 420)
    endif()
    add_test(NAME ${test_name} COMMAND ugurugu_tests)
    set_tests_properties(
        ${test_name}
        PROPERTIES
        ENVIRONMENT
        "QT_QPA_PLATFORM=offscreen;UGURUGU_TEST_SUITE=${suite}"
        TIMEOUT
        ${test_timeout}
    )
    if(WIN32)
        set_tests_properties(
            ${test_name}
            PROPERTIES
            ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
        )
    endif()
endforeach()

if(APPLE OR WIN32)
    add_executable(ugurugu_package_smoke tests/PackageSmoke.cpp)
    target_link_libraries(
        ugurugu_package_smoke
        PRIVATE
        Qt6::Core
        Qt6::Gui
    )
    ugurugu_target_defaults(ugurugu_package_smoke)
endif()

if(APPLE)
    set_target_properties(
        ugurugu_package_smoke
        PROPERTIES
        SKIP_BUILD_RPATH TRUE
    )
    add_custom_command(
        TARGET ugurugu_package_smoke
        POST_BUILD
        COMMAND
        "${CMAKE_COMMAND}"
        "-DSMOKE_EXECUTABLE=$<TARGET_FILE:ugurugu_package_smoke>"
        "-DSMOKE_BUILD_DIRECTORY=${CMAKE_CURRENT_BINARY_DIR}"
        -P
        "${CMAKE_CURRENT_SOURCE_DIR}/tests/PreparePackageSmoke.cmake"
        VERBATIM
    )
    set(
        UGURUGU_PACKAGE_SMOKE_ROOT
        "${CMAKE_CURRENT_BINARY_DIR}/package-smoke"
    )
    set(
        UGURUGU_PACKAGE_SMOKE_APPLICATION
        "${UGURUGU_PACKAGE_SMOKE_ROOT}/install/Ugurugu.app"
    )
    add_custom_target(
        ugurugu_package_smoke_test
        COMMAND
        "${CMAKE_COMMAND}" -E remove_directory
        "${UGURUGU_PACKAGE_SMOKE_ROOT}"
        COMMAND
        "${CMAKE_COMMAND}" --install "${CMAKE_BINARY_DIR}"
        --prefix "${UGURUGU_PACKAGE_SMOKE_ROOT}/install"
        COMMAND
        "${CMAKE_COMMAND}" -E make_directory
        "${UGURUGU_PACKAGE_SMOKE_ROOT}/runtime/MacOS"
        COMMAND
        "${CMAKE_COMMAND}" -E create_symlink
        "${UGURUGU_PACKAGE_SMOKE_APPLICATION}/Contents/Frameworks"
        "${UGURUGU_PACKAGE_SMOKE_ROOT}/runtime/Frameworks"
        COMMAND
        "${CMAKE_COMMAND}" -E copy
        "$<TARGET_FILE:ugurugu_package_smoke>"
        "${UGURUGU_PACKAGE_SMOKE_ROOT}/runtime/MacOS/"
        COMMAND
        "${UGURUGU_PACKAGE_SMOKE_ROOT}/runtime/MacOS/ugurugu_package_smoke"
        "${UGURUGU_PACKAGE_SMOKE_APPLICATION}"
        COMMAND
        /usr/bin/codesign --verify --deep --strict
        "${UGURUGU_PACKAGE_SMOKE_APPLICATION}"
        DEPENDS Ugurugu ugurugu_package_smoke
        USES_TERMINAL
        VERBATIM
    )
elseif(WIN32)
    find_program(
        UGURUGU_POWERSHELL_EXECUTABLE
        NAMES pwsh powershell
        REQUIRED
    )
    set(
        UGURUGU_PACKAGE_SMOKE_ROOT
        "${CMAKE_CURRENT_BINARY_DIR}/package-smoke"
    )
    add_custom_target(
        ugurugu_package_smoke_test
        COMMAND
        "${CMAKE_COMMAND}" -E remove_directory
        "${UGURUGU_PACKAGE_SMOKE_ROOT}"
        COMMAND
        "${CMAKE_COMMAND}" --install "${CMAKE_BINARY_DIR}"
        --config "$<CONFIG>"
        --prefix "${UGURUGU_PACKAGE_SMOKE_ROOT}/install"
        COMMAND
        "${UGURUGU_POWERSHELL_EXECUTABLE}"
        -NoProfile
        -NonInteractive
        -ExecutionPolicy Bypass
        -File "${CMAKE_CURRENT_SOURCE_DIR}/tests/TestWindowsPackage.ps1"
        -PackageRoot "${UGURUGU_PACKAGE_SMOKE_ROOT}/install"
        -SmokeExecutable "$<TARGET_FILE:ugurugu_package_smoke>"
        DEPENDS Ugurugu ugurugu_package_smoke
        USES_TERMINAL
        VERBATIM
    )
endif()
