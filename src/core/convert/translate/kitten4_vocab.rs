//! Kitten4 **编辑器**认识的积木类型清单(**实测导出**,不是推断)。
//!
//! 来源:线上 Kitten4 编辑器(`https://kitten4.codemao.cn/`)页面里
//! `Object.keys(window.Blockly.Blocks).sort()`,共 349 条(2026-09-26)。
//! 编辑器升级后重导一次、整体替换这个数组即可。
//!
//! **为什么需要它**:反向(KN → Kitten4)遇到"一个 KN 名对应多个 Kitten 原类型"的歧义时
//! (如 `change_variables` ← `change_variable` | `change_cloud_variable`),必须挑一个
//! **编辑器真的认识**的名字。挑错的后果实测很重:产物里只要有编辑器不认识的类型,
//! 编辑器加载整份工作区就会失败 —— 画布**一块都不显示**(见 `docs/rounds/34` §4nonies)。

/// **重导流程(编辑器升级后必做)**
///
/// ① 无头浏览器打开 `https://kitten4.codemao.cn/` 并等编辑器就绪(页面有 `window.Blockly`);
/// ② 在页面里取 `Object.keys(window.Blockly.Blocks).sort()`;
/// ③ 用结果**整体替换**下面的数组,并更新本节末尾的"最近一次导出"。
///
/// 判据:产物里出现的类型名(含影子 XML 的 `type`)必须都在这个数组里 ——
/// 少一个,编辑器加载**整份工作区**就会失败(见 `docs/knowledge/convert-semantics.md` §5bis)。
/// 最近一次导出:**2026-09-26,349 条**。
///
/// 编辑器认识的积木类型(已排序,二分查找用)
#[rustfmt::skip]
pub(crate) const KITTEN4_EDITOR_TYPES: &[&str] = &[
    "LOGIC_SHADOW", "SHADOW", "add_width_height_scale", "add_width_height_scale_2",
    "ai_lab_add_data", "ai_lab_classify", "ai_lab_predict_classification", "ai_lab_predict_confidence",
    "allow_rotate", "ask_and_choose", "auto_player_actor_die", "auto_player_actor_is_dead",
    "auto_player_init_new_model", "auto_player_make_decision", "auto_player_on_ai_ready", "auto_player_record_features_for_action",
    "auto_player_record_score_and_delete_actor", "auto_player_run_game_ai", "auto_player_set_action", "auto_player_set_feature",
    "auto_player_train_save_restart", "auto_player_use_model_supervised", "auto_player_use_model_unsupervised", "backdrop_on_change",
    "break", "broadcast_input", "bump", "bump_into_color",
    "calculate", "capture_or_upload_face_pic", "change_cloud_variable", "change_variable",
    "change_volume_or_rate", "change_volume_or_rate_2", "check_hidden", "check_key",
    "check_running_device", "check_screen", "check_sence", "clear_drawing",
    "clone", "cloud_lists_append", "cloud_lists_delete", "cloud_lists_get",
    "cloud_lists_get_value", "cloud_lists_index_of", "cloud_lists_insert_value", "cloud_lists_is_exist",
    "cloud_lists_length", "cloud_lists_replace", "cloud_variables_get", "cloud_variables_set",
    "color_picker", "connected_users_get", "continue_pause_video", "controller_shadow",
    "controls_if", "convert_type", "create_stage_dialog", "default_value",
    "destruct", "display_recognition_result", "dispose", "dispose_clone",
    "divisible_by", "enable_voice_detection", "get", "get_2",
    "get_3", "get_answer", "get_any_midis", "get_audios",
    "get_camera_data", "get_choice", "get_choice_index", "get_choice_or_index",
    "get_clone_index_property", "get_clone_num", "get_current_clone_index", "get_current_costume",
    "get_current_scene", "get_emotion", "get_emotion_result", "get_excel",
    "get_face_age", "get_face_shape", "get_face_shape_result", "get_gender",
    "get_glasses", "get_midis", "get_mobile_rocker_value", "get_mobile_slope_value",
    "get_mouse_info", "get_notes", "get_orientation", "get_physics_property",
    "get_recognition_result", "get_running_device", "get_sensing_current_scene", "get_stage_info",
    "get_time", "get_timer", "get_voice_answer", "get_voice_volume",
    "get_whole_audios", "get_whole_midis", "hide_ranking", "hide_variable",
    "image_stamp", "incompatible_block", "incompatible_output_block", "is_ranking_show_hide",
    "lists_append", "lists_copy", "lists_delete", "lists_get",
    "lists_get_value", "lists_index_of", "lists_insert_value", "lists_is_exist",
    "lists_length", "lists_replace", "logic_boolean", "logic_compare",
    "logic_empty", "logic_negate", "logic_operation", "math_arithmetic",
    "math_modulo", "math_number", "math_number_property", "math_round",
    "math_single", "math_trig", "midi_get", "midi_play_note",
    "midi_play_num_note", "midi_wait", "midimusic_column_tag", "mirror",
    "ml_predict", "ml_probability", "ml_result", "ml_train",
    "mobile_vibrate", "mouse_down", "multiline_text", "on_keydown",
    "on_midimusic_play_columns", "on_midimusic_play_note", "on_mobile_click_btn", "on_mobile_shake",
    "on_mobile_slide", "on_mobile_slope", "on_running_group_activated", "on_shake",
    "on_swipe", "on_tilt", "pen_begin_path", "pen_close_path",
    "physics2_allow_rotate", "physics2_enable_force", "physics2_forbid_bump_with", "physics2_get_property",
    "physics2_set_actor_as", "physics2_set_boundary", "physics2_set_flexibility", "physics2_set_force",
    "physics2_set_force_in_time", "physics2_set_gravity", "physics2_set_mass", "physics2_set_resilience",
    "physics2_set_roughness", "physics2_set_speed", "physics2_set_texture", "play_ask_record",
    "play_audio", "play_audio_2", "play_audio_and_wait", "play_audio_and_wait_2",
    "play_midimusic", "play_midimusic_column", "play_midimusic_till_end", "play_video",
    "play_video_until_end", "play_words_audio", "play_words_audio_wait", "procedures_2_callnoreturn",
    "procedures_2_callreturn", "procedures_2_defnoreturn", "procedures_2_parameter", "procedures_2_return_value",
    "procedures_2_stable_parameter", "procedures_dropdown", "random", "repeat_forever",
    "repeat_forever_until", "repeat_n_times", "reset_timer", "restart",
    "self_appear", "self_ask", "self_ask_listen", "self_ask_record",
    "self_bounce_off_edge", "self_broadcast", "self_broadcast_and_wait", "self_change_coordinate",
    "self_change_effect", "self_change_effect_2", "self_change_effect_3", "self_change_layer",
    "self_change_pen_color_property", "self_change_pen_color_property_2", "self_change_pen_shade", "self_change_pen_size",
    "self_change_pen_size_2", "self_change_position", "self_change_scale", "self_change_scale_2",
    "self_change_size", "self_clear_effects", "self_dialog", "self_dialog_wait",
    "self_disable_physics", "self_disappear", "self_distance_to", "self_enable_angle_constraint",
    "self_enable_physics", "self_face_to", "self_flip", "self_glide_coordinate",
    "self_glide_position", "self_glide_to", "self_go_forward", "self_gradually_appear",
    "self_gradually_disappear", "self_gradually_show_hide", "self_listen", "self_move_specify",
    "self_move_to", "self_next_style", "self_on_tap", "self_out_of_boundary",
    "self_pen_down", "self_pen_up", "self_point_towards", "self_prev_next_style",
    "self_rotate", "self_rotate_around", "self_set_air_friction", "self_set_boundary",
    "self_set_density", "self_set_draggable", "self_set_effect", "self_set_effect_2",
    "self_set_friction", "self_set_gravity", "self_set_mass", "self_set_pen_color",
    "self_set_pen_color_property", "self_set_pen_size", "self_set_position", "self_set_restitution",
    "self_set_role_camp", "self_set_rotation_type", "self_set_static_friction", "self_shake",
    "self_texture", "set_actor_as", "set_camera_opcity", "set_camera_status",
    "set_costume", "set_costume_by_id", "set_costume_by_index", "set_entity_show_hide",
    "set_fill_style", "set_force", "set_force_by_vector", "set_gravity",
    "set_gravity_by_orientation", "set_input_tensor", "set_layer", "set_layer_with_pen",
    "set_midimusic_instrument", "set_midimusic_speed", "set_output_tensor", "set_pen_path",
    "set_scale", "set_scene", "set_scene_by_index", "set_scene_transition",
    "set_speaking_language", "set_theatre_layer", "set_timer_state", "set_top",
    "set_velocity", "set_velocity_by_vector", "set_volume_or_rate", "set_volume_or_rate_2",
    "set_width_height_scale", "shadow_number", "shadow_text", "shock_duration",
    "shock_for_n_times", "show_hide_cloud_list", "show_hide_cloud_variable", "show_hide_list",
    "show_hide_timer", "show_hide_variable", "show_hide_video", "show_ranking",
    "show_stage_dialog", "show_variable", "sprite_on_tap", "stamp",
    "start_as_a_mirror", "start_on_click", "start_on_click_2", "stop",
    "stop_all_audios", "stop_audio_2", "stop_midimusic", "switch_to_screen",
    "sync_tell", "tell", "terminate", "text",
    "text_contain", "text_join", "text_length", "text_select",
    "text_select_changeable", "text_split", "translate", "translate_result",
    "turn_on_the_camera", "turn_on_the_qrcode_camera", "user_id_get", "username_get",
    "variables_get", "variables_set", "voice_recognition", "wait",
    "wait_until", "warp", "when", "wood_block_get",
    "wood_block_set",
];

/// 目标编辑器是否认识这个类型
pub(crate) fn kitten4_editor_knows(kind: &str) -> bool {
    KITTEN4_EDITOR_TYPES.binary_search(&kind).is_ok()
}
