//! Kitten4 **编辑器**认识的积木类型清单(**实测导出**,不是推断)。
//!
//! 来源:线上 Kitten4 编辑器(`https://kitten4.codemao.cn/`)页面里
//! `Object.keys(window.Blockly.Blocks).sort()`;条目数与最近一次导出的日期见
//! [`KITTEN4_VOCAB_EXPORTED`](**单一事实源**,本文件不再另抄一份数字)。
//! 编辑器升级后重导一次、整体替换这个数组并更新它即可。
//!
//! **为什么需要它**:反向(KN → Kitten4)遇到"一个 KN 名对应多个 Kitten 原类型"的歧义时
//! (如 `change_variables` ← `change_variable` | `change_cloud_variable`),必须挑一个
//! **编辑器真的认识**的名字。挑错的后果实测很重:产物里只要有编辑器不认识的类型,
//! 编辑器加载整份工作区就会失败 —— 画布**一块都不显示**(见 `docs/rounds/34` §4nonies)。

/// **重导流程(编辑器升级后必做,可复跑)**
///
/// ① 无头浏览器打开 `https://kitten4.codemao.cn/` 并等编辑器就绪(页面有 `window.Blockly`);
/// ② 在页面控制台执行下面的命令,得到**已排序**、可直接粘进 Rust 数组的行文本:
///    `Object.keys(window.Blockly.Blocks).sort().forEach(k => console.log(\`    "${k}",\`))`
/// ③ 用输出**整体替换**下面的数组,并把 [`KITTEN4_VOCAB_EXPORTED`] 更新为 `(导出日期, 条目数)`。
///
/// 判据:产物里出现的类型名(含影子 XML 的 `type`)必须都在这个数组里 ——
/// 少一个,编辑器加载**整份工作区**就会失败(见 `docs/knowledge/convert-semantics.md` §5bis)。
///
/// 词表**最近一次导出**的单一事实源:`(日期, 条目数)` —— 文件头、[`KITTEN4_EDITOR_TYPES`] 的说明
/// 与新鲜度读数(`tests` 里的 `editor_type_list_freshness_is_reported_not_enforced`)都引用它,
/// 不在注释里另抄一份数字。
///
/// 非测试构建里它只被文档引用(没有代码读它)⇒ 显式 `allow(dead_code)`:仓库的
/// `[lints.rust] unused` 现在是 `allow`,但它迟早会被收紧(见 `docs/rounds/39` §W5④)。
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const KITTEN4_VOCAB_EXPORTED: (&str, usize) = ("2026-09-26", 349);

/// 编辑器认识的积木类型(已排序,二分查找用)
#[rustfmt::skip]
pub(super) const KITTEN4_EDITOR_TYPES: &[&str] = &[
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
pub(super) fn kitten4_editor_knows(kind: &str) -> bool {
    KITTEN4_EDITOR_TYPES.binary_search(&kind).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{KITTEN4_EDITOR_TYPES, KITTEN4_VOCAB_EXPORTED, kitten4_editor_knows};

    /// 表必须**严格升序无重复** —— [`kitten4_editor_knows`] 走 `binary_search`,表一旦无序,
    /// 查询会**静默**答错:该认识的答"不认识" ⇒ 积木被写出阶段剔掉;不认识的答"认识" ⇒
    /// 产物让编辑器**整份加载失败**(`docs/rounds/34` §4nonies)。
    /// 重导词表(文件头的流程)最容易犯这个错,所以钉一条断言。
    #[test]
    fn editor_type_list_is_sorted_and_queryable() {
        assert!(
            KITTEN4_EDITOR_TYPES
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "KITTEN4_EDITOR_TYPES 必须严格升序且无重复(重导后记得 sort)"
        );
        // 二分查找在**首尾与中间**都要真能命中(只查几个点就够:有序性由上面那条守)
        for probe in [
            KITTEN4_EDITOR_TYPES[0],
            KITTEN4_EDITOR_TYPES[KITTEN4_EDITOR_TYPES.len() / 2],
            KITTEN4_EDITOR_TYPES[KITTEN4_EDITOR_TYPES.len() - 1],
            "math_number",
            "get_midis",
            // 反向"还原不出原类型"时的兜底标记(rounds/38),它们必须编辑器认识
            "incompatible_block",
            "incompatible_output_block",
        ] {
            assert!(kitten4_editor_knows(probe), "查不到 `{probe}`");
        }
    }

    /// 新鲜度提醒阈值(天):超过就打印"该重导了"。**只影响打印**,不参与断言。
    const FRESHNESS_WARN_DAYS: u64 = 180;

    /// 词表**新鲜度读数**(`docs/goals/convert-backlog.md` §6.3 的 G4):打印条目数、导出日期与
    /// 距今天数;条目数与 [`KITTEN4_VOCAB_EXPORTED`] 对不上、或导出已超过
    /// [`FRESHNESS_WARN_DAYS`] 天时,**醒目提醒**该重导了(流程见文件头)。
    ///
    /// **刻意只打印、不失败**:按挂钟时间失败的测试会在某个日期之后**自动变红** —— 门就变成噪音、
    /// 人人开始忽略它(仓库的门是 `-D warnings` + 全绿,不能被时间弄红)。所以这里允许它随日期
    /// 改变**打印内容**,但**永远不 panic / assert**;"该重导了"由人读到提醒后按流程处理。
    /// 同理,条目数与记录对不上也**只提醒**:那通常正是"换了语料 / 手工改过数组"的信号,
    /// 但它是给人看的,不是给 CI 判的。
    #[test]
    fn editor_type_list_freshness_is_reported_not_enforced() {
        let (exported_on, exported_count) = KITTEN4_VOCAB_EXPORTED;
        let actual = KITTEN4_EDITOR_TYPES.len();
        let days = days_since(exported_on);
        eprintln!(
            "[词表新鲜度] 条目数 {actual}(导出时记录 {exported_count});最近导出 {exported_on}(距今 {days} 天)"
        );
        if actual != exported_count {
            eprintln!(
                "⚠ [词表新鲜度] 条目数与导出记录**不一致**({actual} vs {exported_count})—— \
                 要么重导后忘了更新 KITTEN4_VOCAB_EXPORTED、要么手工改过数组;重导流程见本文件头"
            );
        }
        if days > FRESHNESS_WARN_DAYS {
            eprintln!(
                "⚠ [词表新鲜度] 导出已 **{days} 天**(> {FRESHNESS_WARN_DAYS})—— 编辑器可能已升级,\
                 **该重导了**(流程见本文件头:`Object.keys(window.Blockly.Blocks).sort()`)"
            );
        }
    }

    /// `YYYY-MM-DD` → 距"现在"的天数(按 **UTC 日界**算,故与本地日历可能差 1 天)。
    /// 用 [`std::time::SystemTime`] ⇒ 返回值随日期变化,但**只用于打印**(见上一条测试的说明)。
    fn days_since(date: &str) -> u64 {
        let mut parts = date.split('-');
        let year: i64 = parts
            .next()
            .expect("导出日期缺 年")
            .parse()
            .expect("年不是数字");
        let month: i64 = parts
            .next()
            .expect("导出日期缺 月")
            .parse()
            .expect("月不是数字");
        let day: i64 = parts
            .next()
            .expect("导出日期缺 日")
            .parse()
            .expect("日不是数字");
        let now_days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("系统时钟早于 1970-01-01")
            .as_secs() as i64
            / 86_400;
        (now_days - civil_to_days(year, month, day)).max(0) as u64
    }

    /// `YYYY-MM-DD` → 自 1970-01-01 起的天数(Howard Hinnant 的 `days_from_civil`)。
    /// 手写而不是引依赖:仓库约定**不引入新第三方依赖**,而标准库没有日历。
    fn civil_to_days(year: i64, month: i64, day: i64) -> i64 {
        let y = year - i64::from(month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400; // [0, 399]
        let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
    }
}
