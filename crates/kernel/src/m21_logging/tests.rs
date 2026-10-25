//! M-21 日志系统的用例集。
//!
//! 覆盖 STAGE-13 §5.2 的验收 **B-1 ~ B-10**（每条至少一条用例），
//! 外加裁决 E / M 相关的补充用例。
//!
//! ⚠️ 本文件里的 CSV 解析器是**手写的极简实现**（RFC 4180 的子集：带引号的字段、
//! 字段内引号成对写出、换行分隔记录）—— **不引入任何依赖**（仓内不新增 crate）。

use super::*;

// ---------------------------------------------------------------------------
// 测试辅助
// ---------------------------------------------------------------------------

/// 极简 CSV 解析器（RFC 4180 子集）。
///
/// * 字段含 `,` / `"` / 换行 ⇒ 用双引号包起来；
/// * 字段内的 `"` 写成 `""`；
/// * `\r\n` 的 `\r` 直接忽略。
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => fields.push(std::mem::take(&mut cur)),
                '\n' => {
                    fields.push(std::mem::take(&mut cur));
                    rows.push(std::mem::take(&mut fields));
                }
                '\r' => {}
                _ => cur.push(c),
            }
        }
    }
    if !cur.is_empty() || !fields.is_empty() {
        fields.push(cur);
        rows.push(fields);
    }
    rows
}

/// 造一条带特殊字符的条目（逗号 / 引号 / 换行 / 中文）—— 转义判别式用。
fn log_special_chars(buf: &mut LogBuffer) {
    buf.log(
        LogLevel::Error,
        "BUS,ACCESS",
        "第一行,含逗号\n第二行含\"引号\"",
        "E_BUS-01",
        42,
        7,
    );
    buf.log(LogLevel::Warn, "ISS-TRAP", "plain message", "", 43, 8);
}

// ---------------------------------------------------------------------------
// B-1 · 五级齐全 · as_str 逐字 · 无 Fatal
// ---------------------------------------------------------------------------

#[test]
fn b1_five_levels_with_exact_as_str() {
    // 穷举匹配（无 `_` 分支）⇒ 级别体系只有这五个（多一个变体就编译不过）。
    let all = [
        LogLevel::Trace,
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Error,
    ];
    assert_eq!(all.len(), 5);
    let names: Vec<&str> = all.iter().map(|l| l.as_str()).collect();
    assert_eq!(names, vec!["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]);
    // 五级序 = rank。
    let ranks: Vec<u8> = all.iter().map(|l| l.rank()).collect();
    assert_eq!(ranks, vec![0, 1, 2, 3, 4]);
    // 派生序与 rank 同向（`PartialOrd` / `Ord` 来自枚举声明序）。
    assert!(LogLevel::Trace < LogLevel::Debug);
    assert!(LogLevel::Debug < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Warn);
    assert!(LogLevel::Warn < LogLevel::Error);
}

#[test]
fn b1_no_fatal_level_exists() {
    // ⚠️ 没有 `Fatal`（致命属 M-22 的 `Severity`，裁决 C）⇒ 最高级就是 Error。
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Error, "X", "y", "", 1, 1);
    let entries = buf.query(&LogQuery::default());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].level, LogLevel::Error);
    assert_eq!(entries[0].level.as_str(), "ERROR");
}

#[test]
fn default_capacities_are_the_wasm_form() {
    // 裁决 E：五级各一带，WASM 形态 4000 / 1000 / 1000 / 500 / 500 = 7000。
    let caps = LevelCapacities::default();
    assert_eq!(caps.trace, 4_000);
    assert_eq!(caps.debug, 1_000);
    assert_eq!(caps.info, 1_000);
    assert_eq!(caps.warn, 500);
    assert_eq!(caps.error, 500);
    assert_eq!(
        caps.trace + caps.debug + caps.info + caps.warn + caps.error,
        7_000
    );
    // `LogLevel::default_capacity` 与之逐级一致。
    assert_eq!(LogLevel::Trace.default_capacity(), 4_000);
    assert_eq!(LogLevel::Debug.default_capacity(), 1_000);
    assert_eq!(LogLevel::Info.default_capacity(), 1_000);
    assert_eq!(LogLevel::Warn.default_capacity(), 500);
    assert_eq!(LogLevel::Error.default_capacity(), 500);
}

// ---------------------------------------------------------------------------
// B-2 · 过滤：低于阈值不入带 · 不计 written · 计 dropped_by_filter
// ---------------------------------------------------------------------------

#[test]
fn b2_below_threshold_is_dropped_and_counted_separately() {
    let mut buf = LogBuffer::new(0);
    assert_eq!(
        buf.level(),
        LogLevel::Trace,
        "默认阈值 = TRACE ⇒ 默认不过滤"
    );

    let old = buf.set_level(LogLevel::Info);
    assert_eq!(old, LogLevel::Trace, "set_level 返回旧阈值");

    buf.log(LogLevel::Trace, "C", "trace msg", "", 10, 1);
    buf.log(LogLevel::Debug, "C", "debug msg", "", 20, 2);
    assert!(buf.is_empty(), "低于阈值 ⇒ 不入带");
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.records_written(), 0, "被过滤的不计 written");
    assert_eq!(buf.dropped_by_filter(), 2, "被过滤的计 dropped_by_filter");

    // 阈值本身保留。
    buf.log(LogLevel::Info, "C", "info msg", "", 30, 3);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf.records_written(), 1);
    assert_eq!(buf.dropped_by_filter(), 2);
}

// ---------------------------------------------------------------------------
// B-3 · 截断：≤128 字节 + truncated=true + UTF-8 字符边界
// ---------------------------------------------------------------------------

#[test]
fn b3_long_ascii_msg_is_truncated_to_128_bytes() {
    let mut buf = LogBuffer::new(0);
    let long = "x".repeat(200);
    buf.log(LogLevel::Info, "C", &long, "", 1, 1);
    let entries = buf.query(&LogQuery::default());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].msg.len(), 128, "落盘 ≤128 字节");
    assert!(entries[0].truncated, "被截 ⇒ truncated = true");
    assert!(long.starts_with(&entries[0].msg));
}

#[test]
fn b3_truncation_never_splits_a_multibyte_char() {
    let mut buf = LogBuffer::new(0);
    // 300 字节的中文（每字 3 字节）⇒ 128 字节处正好落在字符中间。
    let long: String = "中".repeat(100);
    assert_eq!(long.len(), 300);
    buf.log(LogLevel::Info, "C", &long, "", 1, 1);
    let entries = buf.query(&LogQuery::default());
    let msg = &entries[0].msg;

    assert!(entries[0].truncated);
    assert!(msg.len() <= 128, "落盘 ≤128 字节，实际 {}", msg.len());
    assert!(long.starts_with(msg.as_str()), "必须是原文的字符边界前缀");
    // 精确值：能放下多少个完整中文字（128 / 3 ⇒ 42 字 = 126 字节）。
    let mut expected = String::new();
    for ch in long.chars() {
        if expected.len() + ch.len_utf8() > 128 {
            break;
        }
        expected.push(ch);
    }
    assert_eq!(msg, &expected);
    assert_eq!(msg.chars().count(), 42);
    // 末尾不是半个字符（也不是替换字符）。
    assert!(!msg.ends_with('\u{FFFD}'));
    assert_eq!(msg.chars().last(), Some('中'));
}

#[test]
fn b3_truncation_handles_mixed_width_characters() {
    let mut buf = LogBuffer::new(0);
    // ASCII + 三字节中文 + 四字节 emoji 混排。
    let long = format!("{}{}", "a".repeat(100), "中🙂".repeat(50));
    assert!(long.len() > 128);
    buf.log(LogLevel::Info, "C", &long, "", 1, 1);
    let entries = buf.query(&LogQuery::default());
    let msg = &entries[0].msg;
    assert!(entries[0].truncated);
    assert!(msg.len() <= 128);
    assert!(long.starts_with(msg.as_str()));
    // 逐字符走一遍：原文的每个字符边界都在落盘串里（不可能切半个）。
    assert!(msg.is_char_boundary(msg.len()));
    let mut expected = String::new();
    for ch in long.chars() {
        if expected.len() + ch.len_utf8() > 128 {
            break;
        }
        expected.push(ch);
    }
    assert_eq!(msg, &expected);
}

#[test]
fn b3_ctx_and_err_code_survive_msg_truncation() {
    let mut buf = LogBuffer::new(0);
    let long = "y".repeat(300);
    buf.log(LogLevel::Error, "ISS-TRAP", &long, "E_ISS-03", 5, 5);
    let entries = buf.query(&LogQuery::default());
    assert_eq!(entries[0].ctx, "ISS-TRAP", "上下文不因 msg 截断而丢失");
    assert_eq!(entries[0].err_code, "E_ISS-03", "错误码不因 msg 截断而丢失");
    assert!(entries[0].truncated);
    assert_eq!(entries[0].msg.len(), 128);
}

#[test]
fn b3_long_ctx_and_err_code_are_truncated_independently() {
    let mut buf = LogBuffer::new(0);
    let long_ctx = "C".repeat(40);
    let long_code = "E_".repeat(10);
    buf.log(LogLevel::Info, &long_ctx, "short", &long_code, 1, 1);
    let entries = buf.query(&LogQuery::default());
    assert_eq!(entries[0].ctx.len(), 24);
    assert_eq!(entries[0].err_code.len(), 12);
    assert!(entries[0].truncated, "任一字段被截 ⇒ truncated = true");
    // msg 没被截但整体仍然标了 truncated（截的是别的字段）。
    assert_eq!(entries[0].msg, "short");
}

// ---------------------------------------------------------------------------
// B-4 · 溢出：丢最旧 + 该带计数 + 别的带不受影响
// ---------------------------------------------------------------------------

#[test]
fn b4_overflow_drops_oldest_of_that_band_only() {
    let caps = LevelCapacities {
        trace: 2,
        debug: 2,
        info: 2,
        warn: 2,
        error: 2,
    };
    let mut buf = LogBuffer::with_capacities(0, caps);
    buf.log(LogLevel::Trace, "T", "first", "", 1, 1);
    buf.log(LogLevel::Trace, "T", "second", "", 2, 2);
    buf.log(LogLevel::Trace, "T", "third", "", 3, 3);
    buf.log(LogLevel::Debug, "D", "only debug", "", 4, 4);

    assert_eq!(buf.overflow_count_of(LogLevel::Trace), 1, "该带丢一条");
    assert_eq!(buf.overflow_count_of(LogLevel::Debug), 0, "别的带不受影响");
    assert_eq!(buf.overflow_count(), 1);
    assert_eq!(buf.len(), 3, "trace 带 2 条 + debug 带 1 条");
    assert_eq!(buf.records_written(), 4, "入缓冲的条数仍计 4");

    let entries = buf.query(&LogQuery::default());
    let trace_msgs: Vec<&str> = entries
        .iter()
        .filter(|e| e.level == LogLevel::Trace)
        .map(|e| e.msg.as_str())
        .collect();
    assert_eq!(trace_msgs, vec!["second", "third"], "丢的是最旧一条");
}

// ---------------------------------------------------------------------------
// B-5 · query 是纯读
// ---------------------------------------------------------------------------

#[test]
fn b5_query_is_pure_read() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Info, "A", "a", "", 10, 1);
    buf.log(LogLevel::Warn, "B", "b", "E_X", 20, 2);

    let before = buf.query(&LogQuery::default());
    let first = buf.query(&LogQuery::default());
    let second = buf.query(&LogQuery::default());
    assert_eq!(first, second, "连续两次 query 结果相同");
    assert_eq!(first, before);

    // query 之后缓冲内容与计数一条没变。
    assert_eq!(buf.len(), 2);
    assert_eq!(buf.records_written(), 2);
    assert_eq!(buf.dropped_by_filter(), 0);
    assert_eq!(buf.overflow_count(), 0);
    assert_eq!(
        buf.query(&LogQuery::default()),
        before,
        "仍能看到同样的条目"
    );

    // query 之后继续写，旧条目依然可见（读不改变后续可见性）。
    buf.log(LogLevel::Error, "C", "c", "", 30, 3);
    let after = buf.query(&LogQuery::default());
    assert_eq!(after.len(), 3);
    assert!(after.contains(&before[0]));
    assert!(after.contains(&before[1]));
}

// ---------------------------------------------------------------------------
// B-6 · set_level 返回旧阈值 + 切换即时生效
// ---------------------------------------------------------------------------

#[test]
fn b6_set_level_returns_old_threshold_and_takes_effect() {
    let mut buf = LogBuffer::new(0);
    assert_eq!(buf.level(), LogLevel::Trace);
    let old = buf.set_level(LogLevel::Warn);
    assert_eq!(old, LogLevel::Trace, "返回的是旧阈值");
    assert_eq!(buf.level(), LogLevel::Warn, "新阈值立即生效");

    buf.log(LogLevel::Info, "C", "now below", "", 1, 1);
    assert_eq!(buf.len(), 0, "切换后旧阈值不再写入");
    assert_eq!(buf.dropped_by_filter(), 1);

    buf.log(LogLevel::Warn, "C", "at threshold", "", 2, 2);
    assert_eq!(buf.len(), 1, "阈值本身保留");
    let old2 = buf.set_level(LogLevel::Error);
    assert_eq!(old2, LogLevel::Warn);
}

// ---------------------------------------------------------------------------
// B-7 · 导出含全部 8 列 + 虚拟时间非 0
// ---------------------------------------------------------------------------

#[test]
fn b7_csv_export_has_all_eight_columns_and_real_virt_time() {
    let mut buf = LogBuffer::new(3);
    buf.log(LogLevel::Info, "C", "hello", "E_X", 123_456_789, 42);

    let csv = buf.export_csv(&LogQuery::default());
    let rows = parse_csv(&csv);
    assert_eq!(rows.len(), 2, "表头 + 1 行");
    assert_eq!(
        rows[0],
        vec![
            "time_virt_ps",
            "time_host_ms",
            "session_id",
            "level",
            "ctx",
            "err_code",
            "msg",
            "truncated"
        ]
    );
    assert_eq!(rows[1].len(), 8, "每行 8 列");
    assert_eq!(rows[1][0], "123456789", "虚拟时间是真实值，不许恒 0");
    assert_eq!(rows[1][1], "42");
    assert_eq!(rows[1][2], "3");
    assert_eq!(rows[1][3], "INFO");
    assert_eq!(rows[1][7], "false");
}

#[test]
fn b7_json_export_has_all_eight_fields_and_real_virt_time() {
    let mut buf = LogBuffer::new(3);
    buf.log(LogLevel::Warn, "C", "hello", "E_X", 987_654_321, 42);

    let json = buf.export_json(&LogQuery::default());
    let value: serde_json::Value = serde_json::from_str(&json).expect("导出必须是合法 JSON");
    let arr = value.as_array().expect("顶层是数组");
    assert_eq!(arr.len(), 1);
    let obj = arr[0].as_object().expect("元素是对象");
    let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "ctx",
            "err_code",
            "level",
            "msg",
            "session_id",
            "time_host_ms",
            "time_virt_ps",
            "truncated"
        ]
    );
    assert_eq!(obj["time_virt_ps"].as_u64(), Some(987_654_321));
    assert_ne!(obj["time_virt_ps"].as_u64(), Some(0), "虚拟时间不许恒 0");
    assert_eq!(obj["level"].as_str(), Some("WARN"));
    assert_eq!(obj["truncated"].as_bool(), Some(false));
}

// ---------------------------------------------------------------------------
// B-8 · 导出往返：导出 → 解析 → 逐字段等于原条目
// ---------------------------------------------------------------------------

#[test]
fn b8_csv_round_trip_is_field_by_field_equal() {
    let mut buf = LogBuffer::new(9);
    log_special_chars(&mut buf);

    let original = buf.query(&LogQuery::default());
    assert_eq!(original.len(), 2);

    let csv = buf.export_csv(&LogQuery::default());
    let rows = parse_csv(&csv);
    assert_eq!(rows.len(), 3, "表头 + 2 行");
    assert_eq!(rows[1].len(), 8);
    assert_eq!(rows[2].len(), 8);

    for (row, entry) in rows[1..].iter().zip(original.iter()) {
        assert_eq!(row[0], entry.time_virt_ps.to_string());
        assert_eq!(row[1], entry.time_host_ms.to_string());
        assert_eq!(row[2], entry.session_id.to_string());
        assert_eq!(row[3], entry.level.as_str());
        assert_eq!(row[4], entry.ctx, "逗号字段往返相等");
        assert_eq!(row[5], entry.err_code, "错误码往返相等");
        assert_eq!(row[6], entry.msg, "含引号 / 换行的消息往返相等");
        assert_eq!(row[7], if entry.truncated { "true" } else { "false" });
    }
    // 转义确实被触发过（否则这条用例就是空转）。
    assert!(csv.contains("\"BUS,ACCESS\""));
    assert!(csv.contains("\"\"引号\"\""));
}

#[test]
fn b8_json_round_trip_is_field_by_field_equal() {
    let mut buf = LogBuffer::new(9);
    log_special_chars(&mut buf);

    let original = buf.query(&LogQuery::default());
    let json = buf.export_json(&LogQuery::default());
    let value: serde_json::Value = serde_json::from_str(&json).expect("导出必须是合法 JSON");
    let arr = value.as_array().expect("顶层是数组");
    assert_eq!(arr.len(), original.len());

    for (obj, entry) in arr.iter().zip(original.iter()) {
        assert_eq!(obj["time_virt_ps"].as_u64(), Some(entry.time_virt_ps));
        assert_eq!(obj["time_host_ms"].as_u64(), Some(entry.time_host_ms));
        assert_eq!(
            obj["session_id"].as_u64(),
            Some(u64::from(entry.session_id))
        );
        assert_eq!(obj["level"].as_str(), Some(entry.level.as_str()));
        assert_eq!(obj["ctx"].as_str(), Some(entry.ctx.as_str()));
        assert_eq!(obj["err_code"].as_str(), Some(entry.err_code.as_str()));
        assert_eq!(obj["msg"].as_str(), Some(entry.msg.as_str()));
        assert_eq!(obj["truncated"].as_bool(), Some(entry.truncated));
    }
}

#[test]
fn b8_round_trip_preserves_the_truncated_flag() {
    let mut buf = LogBuffer::new(1);
    buf.log(LogLevel::Info, "C", "short", "", 1, 1);
    buf.log(LogLevel::Info, "C", &"z".repeat(300), "", 2, 2);

    let original = buf.query(&LogQuery::default());
    assert!(!original[0].truncated);
    assert!(original[1].truncated);

    let csv = buf.export_csv(&LogQuery::default());
    let rows = parse_csv(&csv);
    assert_eq!(rows[1][7], "false");
    assert_eq!(rows[2][7], "true");
}

// ---------------------------------------------------------------------------
// B-9 · session_id 过滤生效
// ---------------------------------------------------------------------------

#[test]
fn b9_session_id_filter_works() {
    let mut buf = LogBuffer::new(7);
    buf.log(LogLevel::Info, "A", "a", "", 10, 1);
    buf.log(LogLevel::Info, "B", "b", "", 20, 2);

    let all = buf.query(&LogQuery::default());
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|e| e.session_id == 7));
    assert_eq!(buf.session_id(), 7);

    let mine = buf.query(&LogQuery {
        session_id: Some(7),
        ..LogQuery::default()
    });
    assert_eq!(mine.len(), 2, "命中会话 ⇒ 全给");

    let other = buf.query(&LogQuery {
        session_id: Some(8),
        ..LogQuery::default()
    });
    assert!(other.is_empty(), "未命中会话 ⇒ 一条不给");
}

// ---------------------------------------------------------------------------
// B-10 · server_profile 容量数字
// ---------------------------------------------------------------------------

#[test]
fn b10_server_profile_capacities() {
    let caps = LevelCapacities::server_profile();
    assert_eq!(caps.trace, 100_000);
    assert_eq!(caps.debug, 10_000);
    assert_eq!(caps.info, 10_000);
    assert_eq!(caps.warn, 5_000);
    assert_eq!(caps.error, 5_000);
    assert_eq!(caps.of(LogLevel::Trace), 100_000);
    assert_eq!(caps.of(LogLevel::Error), 5_000);
    assert_eq!(
        caps.trace + caps.debug + caps.info + caps.warn + caps.error,
        130_000
    );
}

// ---------------------------------------------------------------------------
// D-134 · `query_recent`：按**虚拟时间**的全局最近
// ---------------------------------------------------------------------------

#[test]
fn query_recent_sorts_by_virtual_time_across_bands() {
    let mut buf = LogBuffer::new(0);
    // 跨带交错写入 ⇒ 带序与时间序**不一致** ⇒ 判别力成立。
    buf.log(LogLevel::Error, "C", "e1", "", 10, 1);
    buf.log(LogLevel::Trace, "C", "t1", "", 20, 2);
    buf.log(LogLevel::Info, "C", "i1", "", 30, 3);
    buf.log(LogLevel::Trace, "C", "t2", "", 40, 4);

    // ⚠️ 既有 `query` 的**形状一个字没变**：仍是「按带升序 + 带内写入先后」。
    let by_band: Vec<String> = buf
        .query(&LogQuery::default())
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(by_band, vec!["t1", "t2", "i1", "e1"], "query 仍按带返回");

    // 新入口：按**虚拟时间**混排（**不是**按带）。
    let by_time: Vec<String> = buf
        .query_recent(&LogQuery::default(), 10)
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(
        by_time,
        vec!["e1", "t1", "i1", "t2"],
        "query_recent 按虚拟时间排（跨带混排）"
    );
    let times: Vec<u64> = buf
        .query_recent(&LogQuery::default(), 10)
        .iter()
        .map(|e| e.time_virt_ps)
        .collect();
    assert_eq!(times, vec![10, 20, 30, 40], "时间戳逐条升序");
}

#[test]
fn query_recent_sorts_by_virtual_time_not_by_host_time() {
    let mut buf = LogBuffer::new(0);
    // ⚠️ 宿主时间与虚拟时间**故意反向** ⇒ 用宿主时间排序会给出**另一个**顺序
    //   ⇒ 这条用例对「拿宿主时间当排序键」的变异**是红的**。
    buf.log(LogLevel::Info, "C", "a", "", 10, 900);
    buf.log(LogLevel::Info, "C", "b", "", 20, 100);
    let got: Vec<String> = buf
        .query_recent(&LogQuery::default(), 10)
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(
        got,
        vec!["a", "b"],
        "按虚拟时间排（宿主时间反向 ⇒ 用宿主时间排会得到 b,a）"
    );
}

#[test]
fn query_recent_ties_are_ordered_by_band_then_insertion() {
    let mut buf = LogBuffer::new(0);
    // 全部**同一虚拟时刻** ⇒ 次序只能靠 (带序, 带内写入序) 决定。
    buf.log(LogLevel::Error, "C", "e1", "", 100, 1);
    buf.log(LogLevel::Trace, "C", "t1", "", 100, 2);
    buf.log(LogLevel::Info, "C", "i1", "", 100, 3);
    buf.log(LogLevel::Trace, "C", "t2", "", 100, 4);
    buf.log(LogLevel::Debug, "C", "d1", "", 100, 5);
    buf.log(LogLevel::Warn, "C", "w1", "", 100, 6);

    // 带序 = trace → debug → info → warn → error；带内 = 写入先后。
    let expected = vec!["t1", "t2", "d1", "i1", "w1", "e1"];
    let got: Vec<String> = buf
        .query_recent(&LogQuery::default(), 10)
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(got, expected, "同刻 ⇒ 带序 + 带内写入序");

    // 两次跑**逐条相同**（确定性）。
    let again: Vec<String> = buf
        .query_recent(&LogQuery::default(), 10)
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(got, again, "同刻次序确定 ⇒ 两次跑逐条相同");
}

#[test]
fn query_recent_n_boundaries() {
    let mut buf = LogBuffer::new(0);
    for i in 0..5u64 {
        buf.log(LogLevel::Info, "C", &format!("m{i}"), "", (i + 1) * 10, i);
    }

    // `n = 0` ⇒ 空。
    assert!(
        buf.query_recent(&LogQuery::default(), 0).is_empty(),
        "n = 0 ⇒ 空"
    );

    // `n > 总数` ⇒ 全部（按序）。
    let all = buf.query_recent(&LogQuery::default(), 99);
    assert_eq!(all.len(), 5);
    let msgs: Vec<&str> = all.iter().map(|e| e.msg.as_str()).collect();
    assert_eq!(msgs, vec!["m0", "m1", "m2", "m3", "m4"]);

    // `n` 命中 ⇒ **最后** n 条（虚拟时间上最新的那些）。
    let last2 = buf.query_recent(&LogQuery::default(), 2);
    let msgs2: Vec<&str> = last2.iter().map(|e| e.msg.as_str()).collect();
    assert_eq!(msgs2, vec!["m3", "m4"], "取的是最后 n 条");
}

#[test]
fn query_recent_respects_the_filter() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Trace, "C", "t", "", 10, 1);
    buf.log(LogLevel::Info, "C", "i", "", 20, 2);
    buf.log(LogLevel::Error, "C", "e", "", 30, 3);

    let warn_plus = buf.query_recent(
        &LogQuery {
            min_level: Some(LogLevel::Warn),
            ..LogQuery::default()
        },
        10,
    );
    assert_eq!(warn_plus.len(), 1);
    assert_eq!(warn_plus[0].msg, "e");

    let in_range = buf.query_recent(
        &LogQuery {
            from_virt_ps: Some(15),
            to_virt_ps: Some(25),
            ..LogQuery::default()
        },
        10,
    );
    assert_eq!(in_range.len(), 1);
    assert_eq!(in_range[0].msg, "i");
}

#[test]
fn query_recent_is_pure_read() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Info, "C", "a", "", 10, 1);
    buf.log(LogLevel::Warn, "C", "b", "E_X", 20, 2);

    let before = buf.query(&LogQuery::default());
    let first = buf.query_recent(&LogQuery::default(), 1);
    let second = buf.query_recent(&LogQuery::default(), 1);
    assert_eq!(first, second, "连续两次结果相同");
    assert_eq!(buf.query(&LogQuery::default()), before, "缓冲内容没变");
    assert_eq!(buf.len(), 2);
    assert_eq!(buf.records_written(), 2);
    assert_eq!(buf.overflow_count(), 0);
}

// ---------------------------------------------------------------------------
// 补充用例（裁决 E / M 与通用语义）
// ---------------------------------------------------------------------------

#[test]
fn query_filters_by_min_level_and_virt_time_range() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Trace, "C", "t", "", 100, 1);
    buf.log(LogLevel::Info, "C", "i", "", 200, 2);
    buf.log(LogLevel::Error, "C", "e", "", 300, 3);

    let warn_plus = buf.query(&LogQuery {
        min_level: Some(LogLevel::Warn),
        ..LogQuery::default()
    });
    assert_eq!(warn_plus.len(), 1);
    assert_eq!(warn_plus[0].msg, "e");

    let in_range = buf.query(&LogQuery {
        from_virt_ps: Some(150),
        to_virt_ps: Some(250),
        ..LogQuery::default()
    });
    assert_eq!(in_range.len(), 1);
    assert_eq!(in_range[0].msg, "i");
}

#[test]
fn query_result_order_is_by_band_then_insertion() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Error, "C", "e1", "", 1, 1);
    buf.log(LogLevel::Trace, "C", "t1", "", 2, 2);
    buf.log(LogLevel::Info, "C", "i1", "", 3, 3);
    buf.log(LogLevel::Trace, "C", "t2", "", 4, 4);

    let got: Vec<String> = buf
        .query(&LogQuery::default())
        .into_iter()
        .map(|e| e.msg)
        .collect();
    assert_eq!(got, vec!["t1", "t2", "i1", "e1"], "按带升序 + 带内写入先后");
}

#[test]
fn clear_keeps_counters() {
    let caps = LevelCapacities {
        trace: 1,
        debug: 1,
        info: 1,
        warn: 1,
        error: 1,
    };
    let mut buf = LogBuffer::with_capacities(0, caps);
    buf.log(LogLevel::Trace, "C", "a", "", 1, 1);
    buf.log(LogLevel::Trace, "C", "b", "", 2, 2);
    buf.set_level(LogLevel::Error);
    buf.log(LogLevel::Info, "C", "dropped", "", 3, 3);

    assert_eq!(buf.overflow_count(), 1);
    assert_eq!(buf.records_written(), 2);
    assert_eq!(buf.dropped_by_filter(), 1);

    buf.clear();
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.overflow_count(), 1, "计数器保留");
    assert_eq!(buf.records_written(), 2, "计数器保留");
    assert_eq!(buf.dropped_by_filter(), 1, "计数器保留");
    assert_eq!(buf.level(), LogLevel::Error, "阈值保留");
}

#[test]
fn log_records_real_virtual_time_from_caller() {
    let mut buf = LogBuffer::new(0);
    buf.log(LogLevel::Info, "C", "m", "", 12_345, 67);
    let entries = buf.query(&LogQuery::default());
    assert_eq!(entries[0].time_virt_ps, 12_345);
    assert_eq!(entries[0].time_host_ms, 67);
    let csv = buf.export_csv(&LogQuery::default());
    assert!(csv.contains("12345,67,"));
    let json = buf.export_json(&LogQuery::default());
    assert!(json.contains("\"time_virt_ps\":12345"));
}

#[test]
fn export_of_empty_buffer_still_has_the_header() {
    let buf = LogBuffer::new(0);
    let csv = buf.export_csv(&LogQuery::default());
    let rows = parse_csv(&csv);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 8);
    let json = buf.export_json(&LogQuery::default());
    assert_eq!(json.trim(), "[]");
}

// ---------------------------------------------------------------------------
// 跨进程形状（契约 v8 · STAGE-29 · D-129 · 裁决 I）
// ---------------------------------------------------------------------------

/// 转换住在 **M-21**（日志条目是**它**拥有的形状）⇒ ⇒ 契约层不依赖内核。
#[test]
fn the_log_entry_dto_mirrors_the_kernel_entry_field_by_field() {
    let entry = LogEntry {
        time_virt_ps: 12_345,
        time_host_ms: 67,
        session_id: 3,
        level: LogLevel::Warn,
        ctx: "m21-logging".to_string(),
        err_code: "E_CTX-07".to_string(),
        msg: "一条日志".to_string(),
        truncated: true,
    };
    let dto = entry.to_dto();
    assert_eq!(dto.time_virt_ps, 12_345);
    assert_eq!(dto.time_host_ms, 67);
    assert_eq!(dto.session_id, 3);
    assert_eq!(dto.level, "WARN", "级别取**内核自己的拼写**（大写）");
    assert_eq!(dto.ctx, "m21-logging");
    assert_eq!(dto.err_code, Some("E_CTX-07".to_string()));
    assert_eq!(dto.msg, "一条日志");
    assert!(dto.truncated);
}

/// ⚠️ **空串 ⇒ `None`**（「没有码」≠「空码」）—— 这一条是 DTO 与内核形状
/// **唯一**的类型差异 ⇒ ⇒ 不钉住它就会把「没有码」传成「空码」。
#[test]
fn an_empty_err_code_becomes_none() {
    let entry = LogEntry {
        time_virt_ps: 0,
        time_host_ms: 0,
        session_id: 0,
        level: LogLevel::Info,
        ctx: "c".to_string(),
        err_code: String::new(),
        msg: "m".to_string(),
        truncated: false,
    };
    assert_eq!(entry.to_dto().err_code, None);
}

/// 五个级别的**拼写**逐个钉住（线上就是这五个字符串 · **没有** `fatal`）。
#[test]
fn every_level_travels_with_the_kernel_spelling() {
    let spellings = [
        (LogLevel::Trace, "TRACE"),
        (LogLevel::Debug, "DEBUG"),
        (LogLevel::Info, "INFO"),
        (LogLevel::Warn, "WARN"),
        (LogLevel::Error, "ERROR"),
    ];
    for (level, spelling) in spellings {
        let entry = LogEntry {
            time_virt_ps: 0,
            time_host_ms: 0,
            session_id: 0,
            level,
            ctx: "c".to_string(),
            err_code: String::new(),
            msg: "m".to_string(),
            truncated: false,
        };
        assert_eq!(entry.to_dto().level, spelling);
    }
}
