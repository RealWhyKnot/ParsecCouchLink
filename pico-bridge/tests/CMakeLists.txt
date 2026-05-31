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
