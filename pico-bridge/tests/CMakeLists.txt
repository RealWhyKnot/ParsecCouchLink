# Host-side unit tests for the pure-C CDC framing. This is a standalone
# project on purpose: it does NOT pull in the Pico SDK or the ARM
# cross-toolchain, so it configures and runs with the host compiler both
# locally and in CI.
#
#   cmake -S pico-bridge/tests -B build-host-tests
#   cmake --build build-host-tests --config Debug
#   ctest --test-dir build-host-tests -C Debug --output-on-failure
cmake_minimum_required(VERSION 3.13)
project(couchlink_host_tests C)

enable_testing()

add_executable(test_cdc_proto
    test_cdc_proto.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/cdc_proto.c
)
target_include_directories(test_cdc_proto PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME cdc_proto COMMAND test_cdc_proto)

add_executable(test_boot_mode_policy
    test_boot_mode_policy.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/boot_mode_policy.c
)
target_include_directories(test_boot_mode_policy PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME boot_mode_policy COMMAND test_boot_mode_policy)

add_executable(test_reply_gate
    test_reply_gate.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/reply_gate.c
)
target_include_directories(test_reply_gate PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME reply_gate COMMAND test_reply_gate)

add_executable(test_dinput_report
    test_dinput_report.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/dinput_report.c
)
target_include_directories(test_dinput_report PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME dinput_report COMMAND test_dinput_report)

add_executable(test_dinput_features
    test_dinput_features.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/dinput.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/dinput_report.c
)
target_include_directories(test_dinput_features PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
)

add_test(NAME dinput_features COMMAND test_dinput_features)

add_executable(test_bt_hid_report
    test_bt_hid_report.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/bt_hid_report.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/dinput_report.c
)
target_include_directories(test_bt_hid_report PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME bt_hid_report COMMAND test_bt_hid_report)

add_executable(test_maple_proto
    test_maple_proto.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/maple_proto.c
)
target_include_directories(test_maple_proto PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME maple_proto COMMAND test_maple_proto)

add_executable(test_usb_packet_debug
    test_usb_packet_debug.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/usb_packet_debug.c
)
target_include_directories(test_usb_packet_debug PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
)

add_test(NAME usb_packet_debug COMMAND test_usb_packet_debug)

add_executable(test_usb_diag
    test_usb_diag.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/usb_diag.c
)
target_include_directories(test_usb_diag PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
)

add_test(NAME usb_diag COMMAND test_usb_diag)

add_executable(test_xgip_constants
    test_xgip_constants.c
)
target_include_directories(test_xgip_constants PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../src)

add_test(NAME xgip_constants COMMAND test_xgip_constants)

set(COUCHLINK_MBEDTLS_DIR "" CACHE PATH "Mbed TLS source tree for the auth tests")
if(NOT COUCHLINK_MBEDTLS_DIR)
    if(DEFINED ENV{PICO_SDK_PATH} AND EXISTS "$ENV{PICO_SDK_PATH}/lib/mbedtls/library/md.c")
        set(COUCHLINK_MBEDTLS_DIR "$ENV{PICO_SDK_PATH}/lib/mbedtls")
    elseif(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../build-_pico_sdk/pico_sdk-src/lib/mbedtls/library/md.c")
        set(COUCHLINK_MBEDTLS_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../build-_pico_sdk/pico_sdk-src/lib/mbedtls")
    else()
        include(FetchContent)
        FetchContent_Declare(couchlink_mbedtls
            URL https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-3.6.2/mbedtls-3.6.2.tar.bz2
            URL_HASH SHA256=8b54fb9bcf4d5a7078028e0520acddefb7900b3e66fec7f7175bb5b7d85ccdca
            SOURCE_SUBDIR unused
        )
        FetchContent_MakeAvailable(couchlink_mbedtls)
        set(COUCHLINK_MBEDTLS_DIR "${couchlink_mbedtls_SOURCE_DIR}")
    endif()
endif()

add_library(couchlink_mbedtls STATIC
    ${COUCHLINK_MBEDTLS_DIR}/library/constant_time.c
    ${COUCHLINK_MBEDTLS_DIR}/library/md.c
    ${COUCHLINK_MBEDTLS_DIR}/library/platform.c
    ${COUCHLINK_MBEDTLS_DIR}/library/platform_util.c
    ${COUCHLINK_MBEDTLS_DIR}/library/sha256.c
)
target_include_directories(couchlink_mbedtls PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${COUCHLINK_MBEDTLS_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
)
target_compile_definitions(couchlink_mbedtls PUBLIC MBEDTLS_CONFIG_FILE="pico_mbedtls_config.h")

add_executable(test_udp_auth
    test_udp_auth.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/udp_auth.c
)
target_link_libraries(test_udp_auth PRIVATE couchlink_mbedtls)

add_test(NAME udp_auth COMMAND test_udp_auth)

add_executable(test_flash_creds
    test_flash_creds.c
    ${CMAKE_CURRENT_SOURCE_DIR}/../src/flash_creds.c
)
target_include_directories(test_flash_creds PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../src
)
target_compile_definitions(test_flash_creds PRIVATE PICO_FLASH_SIZE_BYTES=16384u)

add_test(NAME flash_creds COMMAND test_flash_creds)
