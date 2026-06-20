# Host-side unit tests for the pure-C CDC framing. This is a standalone
# project on purpose: it does NOT pull in the Pico SDK or the ARM
# cross-toolchain, so it configures and runs with the host compiler both
# locally and in CI.
#
#   cmake -S pico-bridge/tests -B build-host-tests
#   cmake --build build-host-tests
#   ctest --test-dir build-host-tests --output-on-failure
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
