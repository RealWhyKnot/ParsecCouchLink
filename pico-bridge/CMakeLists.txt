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

add_executable(pico_bridge
    src/main.c
    src/boot_mode.c
    src/cdc_handlers.c
    src/cdc_proto.c
    src/diag_log.c
    src/fault_handler.c
    src/flash_creds.c
    src/net_udp.c
    src/reset_reason.c
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

message(STATUS "pico-bridge: PICO_BOARD=${PICO_BOARD}, PICO_PLATFORM=${PICO_PLATFORM}")
