install(TARGETS Ugurugu
    BUNDLE DESTINATION .
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
)

if(APPLE)
    set(
        UGURUGU_DOCUMENTATION_DESTINATION
        "Ugurugu.app/Contents/Resources"
    )
elseif(WIN32)
    set(UGURUGU_DOCUMENTATION_DESTINATION ".")
else()
    set(
        UGURUGU_DOCUMENTATION_DESTINATION
        "${CMAKE_INSTALL_DATADIR}/Ugurugu"
    )
endif()
install(
    FILES
    LICENSE
    README.md
    README.en.md
    README.ja.md
    THIRD_PARTY_NOTICES.md
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
)
install(
    FILES resources/licenses/LGPL-3.0.txt
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
)
install(
    FILES resources/fonts/OFL.txt
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
    RENAME Pretendard-OFL.txt
)
install(
    FILES "${spdlog_SOURCE_DIR}/LICENSE"
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
    RENAME spdlog-LICENSE.txt
)
install(
    FILES "${webp_SOURCE_DIR}/COPYING"
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
    RENAME libwebp-LICENSE.txt
)
install(
    FILES "${webp_SOURCE_DIR}/PATENTS"
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
    RENAME libwebp-PATENTS.txt
)
install(
    FILES "${zlib_SOURCE_DIR}/LICENSE"
    DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
    RENAME zlib-LICENSE.txt
)

if(APPLE)
    install(
        FILES "${sparkle_SOURCE_DIR}/LICENSE"
        DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
        RENAME Sparkle-LICENSE.txt
    )
elseif(WIN32)
    install(
        FILES resources/licenses/Velopack-LICENSE.txt
        DESTINATION "${UGURUGU_DOCUMENTATION_DESTINATION}"
        RENAME Velopack-LICENSE.txt
    )
endif()

if(WIN32)
    install(
        FILES
        "${velopack_SOURCE_DIR}/lib/velopack_libc_win_x64_msvc.dll"
        DESTINATION ${CMAKE_INSTALL_BINDIR}
        RENAME velopack_libc.dll
    )
endif()

set(UGURUGU_DEPLOY_TOOL_OPTIONS)
set(UGURUGU_DEPLOY_PLUGIN_OPTIONS)
if(APPLE)
    get_filename_component(
        UGURUGU_QT_PREFIX
        "${Qt6_DIR}/../../.."
        ABSOLUTE
    )
    list(
        APPEND
        UGURUGU_DEPLOY_TOOL_OPTIONS
        "-libpath=${UGURUGU_QT_PREFIX}/lib"
        "-libpath=${sparkle_SOURCE_DIR}"
        "-codesign=-"
    )
    set(UGURUGU_DEPLOY_PLUGIN_OPTIONS NO_PLUGINS)
    set(
        UGURUGU_QT_PLUGIN_ROOT
        "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_PLUGINS}"
    )
    get_filename_component(
        UGURUGU_COCOA_PLUGIN
        "${UGURUGU_QT_PLUGIN_ROOT}/platforms/libqcocoa.dylib"
        REALPATH
    )
    get_filename_component(
        UGURUGU_JPEG_PLUGIN
        "${UGURUGU_QT_PLUGIN_ROOT}/imageformats/libqjpeg.dylib"
        REALPATH
    )
    get_filename_component(
        UGURUGU_MAC_STYLE_PLUGIN
        "${UGURUGU_QT_PLUGIN_ROOT}/styles/libqmacstyle.dylib"
        REALPATH
    )
    install(
        FILES
        "${UGURUGU_COCOA_PLUGIN}"
        DESTINATION
        "Ugurugu.app/Contents/PlugIns/platforms"
    )
    install(
        FILES
        "${UGURUGU_JPEG_PLUGIN}"
        DESTINATION
        "Ugurugu.app/Contents/PlugIns/imageformats"
    )
    install(
        FILES
        "${UGURUGU_MAC_STYLE_PLUGIN}"
        DESTINATION
        "Ugurugu.app/Contents/PlugIns/styles"
    )
    install(
        DIRECTORY
        "${sparkle_SOURCE_DIR}/Sparkle.framework"
        DESTINATION
        "Ugurugu.app/Contents/Frameworks"
        USE_SOURCE_PERMISSIONS
    )
endif()

set(UGURUGU_DEPLOY_RUNTIME_OPTIONS)
if(WIN32)
    # The deploy script's compiler runtime copy depends on windeployqt
    # finding the VC redist directory in the environment and silently skips
    # it otherwise, which shipped releases without MSVCP140_1.dll. The
    # Velopack installer owns the prerequisite instead (--framework
    # vcredist145-x64 in release.yml), which installs the official
    # redistributable when missing and keeps it serviced by Windows Update
    # instead of shipping loose runtime DLLs.
    set(UGURUGU_DEPLOY_RUNTIME_OPTIONS NO_COMPILER_RUNTIME)
endif()

qt_generate_deploy_app_script(
    TARGET Ugurugu
    OUTPUT_SCRIPT ugurugu_deploy_script
    DEPLOY_TOOL_OPTIONS ${UGURUGU_DEPLOY_TOOL_OPTIONS}
    ${UGURUGU_DEPLOY_PLUGIN_OPTIONS}
    ${UGURUGU_DEPLOY_RUNTIME_OPTIONS}
    NO_UNSUPPORTED_PLATFORM_ERROR
)
install(SCRIPT ${ugurugu_deploy_script})

if(WIN32)
    install(
        FILES resources/windows/qt.conf
        DESTINATION ${CMAKE_INSTALL_BINDIR}
    )
endif()
