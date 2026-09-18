include(FetchContent)

set(ZLIB_BUILD_TESTING OFF CACHE BOOL "" FORCE)
set(ZLIB_BUILD_SHARED OFF CACHE BOOL "" FORCE)
set(ZLIB_BUILD_STATIC ON CACHE BOOL "" FORCE)
set(ZLIB_BUILD_MINIZIP OFF CACHE BOOL "" FORCE)
set(ZLIB_INSTALL OFF CACHE BOOL "" FORCE)
# Static Qt builds can carry their own zlib archive.
set(ZLIB_PREFIX ON CACHE BOOL "" FORCE)
FetchContent_Declare(
    zlib
    URL
    "https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz"
    URL_HASH
    "SHA256=bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16"
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(zlib)
set_property(DIRECTORY "${zlib_SOURCE_DIR}" PROPERTY EXCLUDE_FROM_ALL TRUE)
