cmake_minimum_required(VERSION 3.13)

# Default to Pico 2 W (RP2350 + Wi-Fi). Override with -DPICO_BOARD=pico_w
# for the older RP2040 + Wi-Fi board if needed.
set(PICO_BOARD pico2_w CACHE STRING "Target Pico board (pico2_w or pico_w)")

# Locate or fetch the pico-sdk before declaring the project.
include(pico_sdk_import.cmake)

project(pico_bridge C CXX ASM)

set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)

pico_sdk_init()

function(pico_bridge_strip_leading_zeros input output)
    string(REGEX REPLACE "^0+" "" value "${input}")
    if(value STREQUAL "")
        set(value "0")
    endif()
    set(${output} "${value}" PARENT_SCOPE)
endfunction()

set(PICO_BRIDGE_FW_VERSION "" CACHE STRING "Firmware version (YYYY.M.D.N or YYYY.M.D.N-XXXX)")
if(PICO_BRIDGE_FW_VERSION STREQUAL "")
    string(TIMESTAMP PICO_BRIDGE_FW_YEAR_RAW "%Y")
    string(TIMESTAMP PICO_BRIDGE_FW_MONTH_RAW "%m")
    string(TIMESTAMP PICO_BRIDGE_FW_DAY_RAW "%d")
    pico_bridge_strip_leading_zeros("${PICO_BRIDGE_FW_MONTH_RAW}" PICO_BRIDGE_FW_MONTH_RAW)
    pico_bridge_strip_leading_zeros("${PICO_BRIDGE_FW_DAY_RAW}" PICO_BRIDGE_FW_DAY_RAW)
    string(RANDOM LENGTH 4 ALPHABET "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" PICO_BRIDGE_FW_SUFFIX_RAW)
    set(PICO_BRIDGE_FW_VERSION
        "${PICO_BRIDGE_FW_YEAR_RAW}.${PICO_BRIDGE_FW_MONTH_RAW}.${PICO_BRIDGE_FW_DAY_RAW}.0-${PICO_BRIDGE_FW_SUFFIX_RAW}")
endif()

if(NOT PICO_BRIDGE_FW_VERSION MATCHES "^([0-9][0-9][0-9][0-9])\\.([0-9]+)\\.([0-9]+)\\.([0-9]+)(-([A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]))?$")
    message(FATAL_ERROR
        "PICO_BRIDGE_FW_VERSION must be YYYY.M.D.N or YYYY.M.D.N-XXXX, got '${PICO_BRIDGE_FW_VERSION}'")
endif()

pico_bridge_strip_leading_zeros("${CMAKE_MATCH_1}" PICO_BRIDGE_FW_YEAR)
pico_bridge_strip_leading_zeros("${CMAKE_MATCH_2}" PICO_BRIDGE_FW_MONTH)
pico_bridge_strip_leading_zeros("${CMAKE_MATCH_3}" PICO_BRIDGE_FW_DAY)
pico_bridge_strip_leading_zeros("${CMAKE_MATCH_4}" PICO_BRIDGE_FW_REVISION)
set(PICO_BRIDGE_FW_SUFFIX "${CMAKE_MATCH_6}")

if(PICO_BRIDGE_FW_YEAR LESS 2020 OR PICO_BRIDGE_FW_YEAR GREATER 2099)
    message(FATAL_ERROR "Firmware year must be 2020..2099")
endif()
if(PICO_BRIDGE_FW_MONTH LESS 1 OR PICO_BRIDGE_FW_MONTH GREATER 12)
    message(FATAL_ERROR "Firmware month must be 1..12")
endif()
if(PICO_BRIDGE_FW_DAY LESS 1 OR PICO_BRIDGE_FW_DAY GREATER 31)
    message(FATAL_ERROR "Firmware day must be 1..31")
endif()
if(PICO_BRIDGE_FW_REVISION LESS 0 OR PICO_BRIDGE_FW_REVISION GREATER 255)
    message(FATAL_ERROR "Firmware revision must be 0..255")
endif()

if(PICO_BRIDGE_FW_SUFFIX STREQUAL "")
    set(PICO_BRIDGE_FW_SUFFIX_LEN 0)
    set(PICO_BRIDGE_FW_VERSION_STRING
        "${PICO_BRIDGE_FW_YEAR}.${PICO_BRIDGE_FW_MONTH}.${PICO_BRIDGE_FW_DAY}.${PICO_BRIDGE_FW_REVISION}")
else()
    set(PICO_BRIDGE_FW_SUFFIX_LEN 4)
    set(PICO_BRIDGE_FW_VERSION_STRING
        "${PICO_BRIDGE_FW_YEAR}.${PICO_BRIDGE_FW_MONTH}.${PICO_BRIDGE_FW_DAY}.${PICO_BRIDGE_FW_REVISION}-${PICO_BRIDGE_FW_SUFFIX}")
endif()

math(EXPR PICO_BRIDGE_USB_BCD_MONTH_TENS "${PICO_BRIDGE_FW_MONTH} / 10")
math(EXPR PICO_BRIDGE_USB_BCD_MONTH_ONES "${PICO_BRIDGE_FW_MONTH} % 10")
math(EXPR PICO_BRIDGE_USB_BCD_DAY_TENS "${PICO_BRIDGE_FW_DAY} / 10")
math(EXPR PICO_BRIDGE_USB_BCD_DAY_ONES "${PICO_BRIDGE_FW_DAY} % 10")
set(PICO_BRIDGE_USB_BCD_DEVICE
    "0x${PICO_BRIDGE_USB_BCD_MONTH_TENS}${PICO_BRIDGE_USB_BCD_MONTH_ONES}${PICO_BRIDGE_USB_BCD_DAY_TENS}${PICO_BRIDGE_USB_BCD_DAY_ONES}")

set(PICO_BRIDGE_GENERATED_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
configure_file(
    "${CMAKE_CURRENT_LIST_DIR}/src/build_version.h.in"
    "${PICO_BRIDGE_GENERATED_DIR}/pico_bridge_build_version.h"
    @ONLY
)

add_executable(pico_bridge
    src/main.c
    src/boot_mode.c
    src/cdc_handlers.c
    src/cdc_proto.c
    src/diag_log.c
    src/fault_handler.c
    src/flash_creds.c
    src/heartbeat.c
    src/net_udp.c
    src/reset_reason.c
    src/usb_diag.c
    src/usb_descriptors.c
    src/watchdog.c
    src/wifi.c
    src/xinput.c
)

# Wi-Fi country code. WORLDWIDE is the safest pre-built default; EU/UK
# builders who need channels 12 / 13 can override at configure time:
#   cmake -S pico-bridge -B build -DPICO_BRIDGE_WIFI_COUNTRY=CYW43_COUNTRY_UK
# Use any of the CYW43_COUNTRY_* macros from pico-sdk's cyw43_arch.h.
set(PICO_BRIDGE_WIFI_COUNTRY "CYW43_COUNTRY_WORLDWIDE"
    CACHE STRING "CYW43 country code (cyw43_arch_init_with_country)")
target_compile_definitions(pico_bridge PRIVATE
    PICO_BRIDGE_WIFI_COUNTRY=${PICO_BRIDGE_WIFI_COUNTRY}
)

target_include_directories(pico_bridge PRIVATE
    ${PICO_BRIDGE_GENERATED_DIR}
    ${CMAKE_CURRENT_LIST_DIR}/src
)

target_link_libraries(pico_bridge
    pico_stdlib
    pico_unique_id
    pico_flash          # flash_safe_execute (coordinates core 1 + IRQs)
    hardware_flash
    hardware_sync
    tinyusb_device
    pico_cyw43_arch_lwip_threadsafe_background
)

# Use UART for any stdio (e.g. for early-boot prints); USB stdio is
# avoided because we own the USB device.
pico_enable_stdio_usb(pico_bridge 0)
pico_enable_stdio_uart(pico_bridge 1)

pico_add_extra_outputs(pico_bridge)

message(STATUS "pico-bridge: PICO_BOARD=${PICO_BOARD}, PICO_PLATFORM=${PICO_PLATFORM}, FW_VERSION=${PICO_BRIDGE_FW_VERSION_STRING}")
