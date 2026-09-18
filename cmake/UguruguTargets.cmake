if(MSVC)
    set_property(
        SOURCE src/app/Logging.cpp
        APPEND
        PROPERTY COMPILE_OPTIONS /wd4702
    )
endif()

add_library(ugurugu_core STATIC ${UGURUGU_CORE_SOURCES})
target_include_directories(ugurugu_core PUBLIC src)
# Deliberately no Qt6::Widgets. The document, render and IO core must stay
# usable from a non-widget UI, so anything needing QtWidgets belongs in
# ugurugu_ui instead.
target_link_libraries(
    ugurugu_core
    PUBLIC
    Qt6::Core
    Qt6::Gui
    spdlog::spdlog
    PRIVATE
    ZLIB::ZLIBSTATIC
    webp
    libwebpmux
)
ugurugu_target_defaults(ugurugu_core)

add_library(ugurugu_ui STATIC ${UGURUGU_UI_SOURCES})
ugurugu_target_defaults(ugurugu_ui)
target_include_directories(ugurugu_ui PUBLIC src)
target_link_libraries(
    ugurugu_ui
    PUBLIC
    ugurugu_core
    Qt6::Concurrent
    Qt6::Widgets
    PRIVATE
    # The QRhi headers behind CanvasFrameView are semi-public: stable C++ API,
    # but installed with the private headers.
    Qt6::GuiPrivate
)
qt_add_shaders(ugurugu_ui "ugurugu_canvas_shaders"
    PREFIX "/shaders"
    BASE "resources/shaders"
    FILES
    resources/shaders/canvas_frame.vert
    resources/shaders/canvas_frame.frag
)

if(APPLE)
    target_sources(
        ugurugu_ui
        PRIVATE
        src/ui/MacWindowChrome.hpp
        src/ui/MacWindowChrome.mm
    )
    set_source_files_properties(
        src/ui/MacWindowChrome.mm
        PROPERTIES
        COMPILE_OPTIONS "-fobjc-arc"
    )
    target_link_libraries(ugurugu_ui PUBLIC "-framework AppKit")
endif()

qt_add_executable(
    Ugurugu
    src/app/UpdateController.hpp
    ${UGURUGU_UPDATE_SOURCE}
    src/main.cpp
)

if(APPLE)
    set_source_files_properties(
        resources/icons/Ugurugu.icns
        PROPERTIES
        MACOSX_PACKAGE_LOCATION Resources
    )
    target_sources(
        Ugurugu
        PRIVATE
        resources/icons/Ugurugu.icns
    )
    set_source_files_properties(
        src/app/UpdateControllerMac.mm
        PROPERTIES
        COMPILE_OPTIONS "-fobjc-arc"
    )
    configure_file(
        resources/macos/Info.plist.in
        "${CMAKE_CURRENT_BINARY_DIR}/Ugurugu-Info.plist"
        @ONLY
    )
elseif(WIN32)
    configure_file(
        resources/icons/Ugurugu.rc.in
        "${CMAKE_CURRENT_BINARY_DIR}/Ugurugu.rc"
        @ONLY
    )
    target_sources(
        Ugurugu
        PRIVATE
        "${CMAKE_CURRENT_BINARY_DIR}/Ugurugu.rc"
    )
endif()

qt_add_translation(
    UGURUGU_QM_FILES
    i18n/ugurugu_ko.ts
    i18n/ugurugu_ja.ts
)
qt_add_resources(Ugurugu "ugurugu_translations"
    PREFIX "/i18n"
    BASE "${CMAKE_CURRENT_BINARY_DIR}"
    FILES ${UGURUGU_QM_FILES}
)
qt_add_resources(Ugurugu "ugurugu_fonts"
    PREFIX "/fonts"
    BASE "resources/fonts"
    FILES resources/fonts/PretendardJP-Medium.otf
)

target_link_libraries(
    Ugurugu
    PRIVATE
    ugurugu_ui
    ${UGURUGU_UPDATE_LIBRARIES}
)
if(WIN32)
    # Qt 6.11 exposes WinTab as a private Windows application interface. The
    # distribution build pins Qt exactly, so enabling the Wacom compatibility
    # path here is preferable to duplicating Qt's WinTab packet handling.
    target_link_libraries(Ugurugu PRIVATE Qt6::GuiPrivate)
endif()
target_compile_definitions(
    Ugurugu
    PRIVATE
    UGURUGU_VERSION="${PROJECT_VERSION}"
)
ugurugu_target_defaults(Ugurugu)

set_target_properties(Ugurugu PROPERTIES
    WIN32_EXECUTABLE TRUE
    MACOSX_BUNDLE TRUE
    MACOSX_BUNDLE_BUNDLE_NAME "Ugurugu"
    MACOSX_BUNDLE_ICON_FILE "Ugurugu.icns"
    MACOSX_BUNDLE_GUI_IDENTIFIER "dev.ugurugu.app"
    MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
    MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
)

if(APPLE)
    set_target_properties(
        Ugurugu
        PROPERTIES
        MACOSX_BUNDLE_INFO_PLIST
        "${CMAKE_CURRENT_BINARY_DIR}/Ugurugu-Info.plist"
        INSTALL_RPATH
        "@executable_path/../Frameworks"
    )
endif()
