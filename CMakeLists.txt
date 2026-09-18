cmake_minimum_required(VERSION 3.31)

if(APPLE AND NOT CMAKE_OSX_DEPLOYMENT_TARGET)
    set(
        CMAKE_OSX_DEPLOYMENT_TARGET
        "14.0"
        CACHE STRING
        "Minimum supported macOS version"
        FORCE
    )
endif()

project(Ugurugu VERSION 2.2.10 LANGUAGES C CXX)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

include(GNUInstallDirs)
include(CTest)
include(UguruguBuildSettings)
include(UguruguCompression)
if(EMSCRIPTEN)
    include(UguruguSources)
    include(UguruguWasm)
    return()
endif()
include(UguruguDependencies)
include(UguruguSources)
include(UguruguTargets)
include(UguruguPackaging)
include(UguruguTests)
include(UguruguFuzzing)
include(UguruguTools)
