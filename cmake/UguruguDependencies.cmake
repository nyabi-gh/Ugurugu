include(FetchContent)

find_package(
    Qt6 6.10
    REQUIRED
    COMPONENTS
    Core
    Concurrent
    Gui
    GuiPrivate
    Widgets
    LinguistTools
    ShaderTools
)

set(
    UGURUGU_DISTRIBUTION_QT_VERSION
    ""
    CACHE STRING
    "Exact Qt version required for distribution builds"
)
if(UGURUGU_DISTRIBUTION_QT_VERSION
    AND NOT Qt6_VERSION VERSION_EQUAL UGURUGU_DISTRIBUTION_QT_VERSION
)
    message(
        FATAL_ERROR
        "Distribution builds require Qt "
        "${UGURUGU_DISTRIBUTION_QT_VERSION}, found ${Qt6_VERSION}"
    )
endif()

set(SPDLOG_BUILD_EXAMPLE OFF CACHE BOOL "" FORCE)
set(SPDLOG_BUILD_SHARED OFF CACHE BOOL "" FORCE)
set(SPDLOG_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(SPDLOG_BUILD_BENCH OFF CACHE BOOL "" FORCE)
set(SPDLOG_FMT_EXTERNAL OFF CACHE BOOL "" FORCE)
set(SPDLOG_FMT_EXTERNAL_HO OFF CACHE BOOL "" FORCE)
if(WIN32)
    set(SPDLOG_WCHAR_FILENAMES ON CACHE BOOL "" FORCE)
endif()
FetchContent_Declare(
    spdlog
    URL
    "https://github.com/gabime/spdlog/archive/refs/tags/v1.16.0.tar.gz"
    URL_HASH
    "SHA256=8741753e488a78dd0d0024c980e1fb5b5c85888447e309d9cb9d949bdb52aa3e"
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(spdlog)

set(WEBP_BUILD_ANIM_UTILS OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_CWEBP OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_DWEBP OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_GIF2WEBP OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_IMG2WEBP OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_VWEBP OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_WEBPINFO OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_WEBPMUX OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_EXTRAS OFF CACHE BOOL "" FORCE)
set(WEBP_BUILD_LIBWEBPMUX ON CACHE BOOL "" FORCE)
FetchContent_Declare(
    webp
    URL
    "https://github.com/webmproject/libwebp/archive/refs/tags/v1.6.0.tar.gz"
    URL_HASH
    "SHA256=93a852c2b3efafee3723efd4636de855b46f9fe1efddd607e1f42f60fc8f2136"
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
set(UGURUGU_AUTOMOC "${CMAKE_AUTOMOC}")
set(UGURUGU_AUTOUIC "${CMAKE_AUTOUIC}")
set(UGURUGU_AUTORCC "${CMAKE_AUTORCC}")
set(CMAKE_AUTOMOC OFF)
set(CMAKE_AUTOUIC OFF)
set(CMAKE_AUTORCC OFF)
FetchContent_MakeAvailable(webp)
set(CMAKE_AUTOMOC "${UGURUGU_AUTOMOC}")
set(CMAKE_AUTOUIC "${UGURUGU_AUTOUIC}")
set(CMAKE_AUTORCC "${UGURUGU_AUTORCC}")
unset(UGURUGU_AUTOMOC)
unset(UGURUGU_AUTOUIC)
unset(UGURUGU_AUTORCC)
set_property(DIRECTORY "${webp_SOURCE_DIR}" PROPERTY EXCLUDE_FROM_ALL TRUE)

qt_standard_project_setup()

set(UGURUGU_UPDATE_SOURCE src/app/UpdateControllerStub.cpp)
set(UGURUGU_UPDATE_LIBRARIES)

if(APPLE)
    enable_language(OBJCXX)
    FetchContent_Declare(
        sparkle
        URL
        "https://github.com/sparkle-project/Sparkle/releases/download/2.9.6/Sparkle-2.9.6.tar.xz"
        URL_HASH
        "SHA256=52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192"
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
    )
    FetchContent_MakeAvailable(sparkle)
    find_library(
        UGURUGU_SPARKLE_FRAMEWORK
        Sparkle
        PATHS "${sparkle_SOURCE_DIR}"
        NO_DEFAULT_PATH
        REQUIRED
    )
    set(
        UGURUGU_UPDATE_SOURCE
        src/app/UpdateControllerMac.mm
    )
    list(
        APPEND
        UGURUGU_UPDATE_LIBRARIES
        "${UGURUGU_SPARKLE_FRAMEWORK}"
    )
elseif(WIN32)
    FetchContent_Declare(
        velopack
        URL
        "https://github.com/velopack/velopack/releases/download/1.2.0/velopack_libc_1.2.0.zip"
        URL_HASH
        "SHA256=547262ed7a1ab1ff62f580aa53851ede2f1a451ac61b8974eb7bc01117488835"
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
    )
    FetchContent_MakeAvailable(velopack)
    add_library(velopack_libc SHARED IMPORTED GLOBAL)
    set_target_properties(
        velopack_libc
        PROPERTIES
        IMPORTED_LOCATION
        "${velopack_SOURCE_DIR}/lib/velopack_libc_win_x64_msvc.dll"
        IMPORTED_IMPLIB
        "${velopack_SOURCE_DIR}/lib/velopack_libc_win_x64_msvc.dll.lib"
        INTERFACE_INCLUDE_DIRECTORIES
        "${velopack_SOURCE_DIR}/include"
    )
    set(
        UGURUGU_UPDATE_SOURCE
        src/app/UpdateControllerWindows.cpp
    )
    list(
        APPEND
        UGURUGU_UPDATE_LIBRARIES
        Qt6::Concurrent
        velopack_libc
    )
endif()
