#include "maple_output.h"

#include "diag_log.h"
#include "gamepad_state.h"

static maple_map_mode_t current_mode = MAPLE_MAP_STANDARD;
static maple_controller_condition_t current_condition;

static gamepad_state_t snapshot_gamepad_state(void) {
    gamepad_state_t snapshot;
    snapshot.buttons = g_gamepad_state.buttons;
    snapshot.left_trigger = g_gamepad_state.left_trigger;
    snapshot.right_trigger = g_gamepad_state.right_trigger;
    snapshot.left_x = g_gamepad_state.left_x;
    snapshot.left_y = g_gamepad_state.left_y;
    snapshot.right_x = g_gamepad_state.right_x;
    snapshot.right_y = g_gamepad_state.right_y;
    return snapshot;
}

void maple_output_init(maple_map_mode_t mode) {
    current_mode = mode;
    gamepad_state_t neutral = {0};
    current_condition = maple_translate_xinput(&neutral, current_mode);
    diag_log_msg("run: Maple controller responder ready");
}

void maple_output_task(void) {
    gamepad_state_t snapshot = snapshot_gamepad_state();
    current_condition = maple_translate_xinput(&snapshot, current_mode);
}

maple_controller_condition_t maple_output_current_condition(void) {
    return current_condition;
}

size_t maple_output_build_response(const maple_request_t *request, uint8_t *out, size_t out_cap) {
    gamepad_state_t snapshot = snapshot_gamepad_state();
    return maple_build_response(request, &snapshot, current_mode, out, out_cap);
}
