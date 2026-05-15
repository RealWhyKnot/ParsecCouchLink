# Standard CMake boilerplate to locate or fetch the pico-sdk.
#
# Resolution order (first match wins):
#   1. $PICO_SDK_PATH environment variable
#   2. $PICO_SDK_FETCH_FROM_GIT (clone into build dir at configure time)
#   3. error
#
# The contents below are based on the canonical pico_sdk_import.cmake
# shipped in the pico-sdk repo, simplified for direct use here.

if (DEFINED ENV{PICO_SDK_PATH} AND (NOT PICO_SDK_PATH))
    set(PICO_SDK_PATH $ENV{PICO_SDK_PATH})
    message(STATUS "Using PICO_SDK_PATH from environment: ${PICO_SDK_PATH}")
endif ()

if (NOT PICO_SDK_PATH)
    if (PICO_SDK_FETCH_FROM_GIT OR DEFINED ENV{PICO_SDK_FETCH_FROM_GIT})
        include(FetchContent)
        set(FETCHCONTENT_BASE_DIR_save ${FETCHCONTENT_BASE_DIR})
        set(FETCHCONTENT_BASE_DIR ${CMAKE_BINARY_DIR}/_pico_sdk)
        set(PICO_SDK_FETCH_FROM_GIT_TAG "2.1.0" CACHE STRING "pico-sdk git tag")
        FetchContent_Declare(
            pico_sdk
            GIT_REPOSITORY https://github.com/raspberrypi/pico-sdk.git
            GIT_TAG ${PICO_SDK_FETCH_FROM_GIT_TAG}
            GIT_SUBMODULES_RECURSE TRUE
        )
        FetchContent_GetProperties(pico_sdk)
        if (NOT pico_sdk_POPULATED)
            message(STATUS "Fetching pico-sdk @ ${PICO_SDK_FETCH_FROM_GIT_TAG}")
            FetchContent_Populate(pico_sdk)
            set(PICO_SDK_PATH ${pico_sdk_SOURCE_DIR})
        endif ()
        set(FETCHCONTENT_BASE_DIR ${FETCHCONTENT_BASE_DIR_save})
    else ()
        message(FATAL_ERROR
                "PICO_SDK_PATH not set and PICO_SDK_FETCH_FROM_GIT not requested.\n"
                "Set PICO_SDK_PATH to your pico-sdk checkout, or pass\n"
                "  -DPICO_SDK_FETCH_FROM_GIT=ON\n"
                "to cmake. See scripts/build.ps1 for a one-shot helper.")
    endif ()
endif ()

get_filename_component(PICO_SDK_PATH "${PICO_SDK_PATH}" REALPATH BASE_DIR "${CMAKE_BINARY_DIR}")
if (NOT EXISTS ${PICO_SDK_PATH})
    message(FATAL_ERROR "Directory '${PICO_SDK_PATH}' (PICO_SDK_PATH) does not exist")
endif ()

set(PICO_SDK_INIT_CMAKE_FILE ${PICO_SDK_PATH}/pico_sdk_init.cmake)
if (NOT EXISTS ${PICO_SDK_INIT_CMAKE_FILE})
    message(FATAL_ERROR "Directory '${PICO_SDK_PATH}' does not contain pico_sdk_init.cmake")
endif ()

set(PICO_SDK_PATH ${PICO_SDK_PATH} CACHE PATH "Path to the Raspberry Pi Pico SDK" FORCE)

include(${PICO_SDK_INIT_CMAKE_FILE})
