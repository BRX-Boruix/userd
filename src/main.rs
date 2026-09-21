//! BORUIX `userd`：用户态账户守护进程（ADR-040 §2.8 / A1-8）。
//!
//! 独立用户态进程（非内核 crate，ADR-002 可替换组件边界），形状同 `volumed`/
//! `driverd`：由 init 经 `exec_path("/programs/userd.elf", &[])` 派生，启动
//! 失败非致命（/users 为空目录时系统仍可用，无「半可用」伪状态）。
//!
//! **职责**（ADR-040 §2.8，账户即文件——内核不解析账户表，Q6 裁决）：
//! 1. 读 `/config/users.json`（用户态解析；schema 见 `parse_accounts`——账户表
//!    不存在时如实跳过并驻留等待，不伪造空账户）。
//! 2. 为每个账户 `mkdir /users/<name>`（已存在容忍 `AlreadyExists`——幂等），
//!    并以 `chown` 把家目录属主设为 (uid, gid)（A1-7 chown 落位后的第一个
//!    真实消费者）、`chmod` 设 0700（家目录归用户私有）。
//! 3. 维护 `/users/<name>/identity` 只读投影：JSON `{"uid":..,"gid":..,"name":".."}`
//!    （ADR-013 自解释）；内容未变不重写（幂等，无写放大）。
//! 4. **对账循环**：周期（10s）重读账户表同步——新增账户补建；表里已删除的
//!    账户**不删除家目录**（数据安全：删账户≠删数据，归管理员工具，不越权）。
//!
//! **不新增 syscall**（Q6）：全部经普通 VFS syscall 完成。

#![no_std]
#![no_main]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use libsys::*;

/// 账户记录（/config/users.json 的 users[] 元素）。
struct Account {
    name: String,
    uid: u32,
    gid: u32,
}

/// 向 STDOUT 输出一行日志（`[userd] ...`）。
fn log(msg: &[u8]) {
    let _ = write(STDOUT, b"[userd] ");
    let _ = write(STDOUT, msg);
    let _ = write(STDOUT, b"\n");
}

/// 带格式的日志（alloc::format 动态拼接，可含数字/错误）。
fn logf(args: core::fmt::Arguments) {
    let s = alloc::format!("{}", args);
    log(s.as_bytes());
}

/// 解析 /config/users.json 字节流为账户列表。
///
/// schema：`{"users":[{"name":"alice","uid":1000,"gid":1000}, ...]}`。
/// 未知字段跳过；名字非空、数值可解析才收录（畸形条目**如实跳过并记录**，
/// 不 panic——账户表是用户态文件，内容不可信）。
fn parse_accounts(bytes: &[u8]) -> Vec<Account> {
    let mut out = Vec::new();
    let Ok(text) = core::str::from_utf8(bytes) else {
        log(b"users.json not UTF-8; skipped");
        return out;
    };
    let mut p = libsys::json::JsonParser::new(text);
    let Ok(parsed) = p.parse() else {
        log(b"users.json malformed JSON; skipped");
        return out;
    };
    if let libsys::json::JsonValue::Object(fields) = parsed {
        for (k, v) in fields {
            if k != "users" {
                continue;
            }
            if let libsys::json::JsonValue::Array(items) = v {
                for it in items {
                    if let libsys::json::JsonValue::Object(obj) = it {
                        let mut name = String::from("");
                        let mut uid: Option<u32> = None;
                        let mut gid: Option<u32> = None;
                        for (fk, fv) in obj {
                            match fk.as_str() {
                                "name" => {
                                    if let libsys::json::JsonValue::String(s) = fv {
                                        name = s;
                                    }
                                }
                                "uid" => {
                                    if let libsys::json::JsonValue::Number(n) = fv {
                                        uid = n.parse::<u32>().ok();
                                    }
                                }
                                "gid" => {
                                    if let libsys::json::JsonValue::Number(n) = fv {
                                        gid = n.parse::<u32>().ok();
                                    }
                                }
                                _ => {}
                            }
                        }
                        // 名字合法性：非空、非路径成分（/ 为路径分隔，绝不能进名字）。
                        let bad = name.is_empty()
                            || name == "."
                            || name == ".."
                            || name.contains('/');
                        if bad {
                            log(b"account with empty/invalid name; skipped");
                            continue;
                        }
                        if let (Some(u), Some(g)) = (uid, gid) {
                            out.push(Account { name, uid: u, gid: g });
                        } else {
                            logf(format_args!("account {} missing uid/gid; skipped", name));
                        }
                    }
                }
            }
        }
    }
    out
}

/// 构造 /users/<name>/identity 投影内容（JsonWriter 负责转义——账户名已禁 /，
/// 引号与反斜杠由 field_str 的转义路径处理）。
fn identity_json(name: &str, uid: u32, gid: u32) -> String {
    let mut target = libsys::json::VecTarget::new();
    let mut writer = libsys::json::JsonWriter::new(&mut target);
    if let Ok(mut obj) = writer.start_object() {
        let _ = obj.field_u64("uid", uid as u64);
        let _ = obj.field_u64("gid", gid as u64);
        let _ = obj.field_str("name", name);
        let _ = obj.end();
    }
    target.into_string().unwrap_or_else(|_| String::from("{}"))
}

/// 读取文件全部字节（不存在/读失败返回 None）。
fn read_file(path: &str) -> Option<Vec<u8>> {
    let fd = open(path, OpenFlags::READ_ONLY, Permissions::readonly()).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        match read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(_) => {
                let _ = close(fd);
                return None;
            }
        }
    }
    let _ = close(fd);
    Some(data)
}

/// 幂等写文件（create/truncate + write + close）。
fn write_file(path: &str, data: &[u8]) -> Result<(), Error> {
    let fd = open(path, OpenFlags::CREATE_OR_TRUNCATE, Permissions::read_write())?;
    let mut off = 0usize;
    while off < data.len() {
        let n = write(fd, &data[off..])?;
        if n == 0 {
            let _ = close(fd);
            return Err(Error::Io);
        }
        off += n;
    }
    let _ = close(fd);
    Ok(())
}

/// 为单个账户同步家目录（mkdir + chown + chmod + identity 投影）。幂等。
fn ensure_home(acc: &Account) {
    let mut path = String::from("/users/");
    path.push_str(&acc.name);
    match mkdir(&path, Permissions::all()) {
        Ok(_) => {}
        // 已存在：幂等容忍。
        Err(Error::AlreadyExists) => {}
        Err(e) => {
            logf(format_args!("mkdir {} failed: {:?}", path, e));
            return;
        }
    }
    // 家目录属主 = 账户身份（A1-7 chown 第一个真实消费者）。
    if let Err(e) = chown(&path, acc.uid, acc.gid) {
        logf(format_args!("chown {} failed: {:?}", path, e));
    }
    // 0700：家目录归用户私有（userd 以 System/全能力运行，可越 classic 段写）。
    if let Err(e) = chmod(&path, 0o700) {
        logf(format_args!("chmod {} failed: {:?}", path, e));
    }
    // identity 只读投影：内容变化才重写（幂等，无写放大）。
    let mut ipath = path.clone();
    ipath.push_str("/identity");
    let want = identity_json(&acc.name, acc.uid, acc.gid);
    let stale = match read_file(&ipath) {
        Some(cur) => cur != want.as_bytes(),
        // 投影不存在视为待写。家目录属主是账户 (uid,gid)，userd（uid1）classic
        // 评估不过——init 派生身份即全能力（A1-2），CAP_OWNER 越过 classic 段成立。
        None => true,
    };
    if stale {
        if let Err(e) = write_file(&ipath, want.as_bytes()) {
            logf(format_args!("write {} failed: {:?}", ipath, e));
        }
    }
    logf(format_args!("home ready: {} ({}:{})", path, acc.uid, acc.gid));
}

/// 对账一轮：读账户表 → 逐账户 ensure_home。
/// 账户表缺失 → 如实记录并等下一轮（不伪造空账户，不退出——表可能稍后出现）。
fn reconcile() {
    let bytes = match read_file("/config/users.json") {
        Some(b) => b,
        None => {
            log(b"no /config/users.json yet; waiting");
            return;
        }
    };
    let accounts = parse_accounts(&bytes);
    if accounts.is_empty() {
        log(b"users.json empty/malformed; nothing to do");
        return;
    }
    for acc in &accounts {
        ensure_home(acc);
    }
}


// ---------------------------------------------------------------------------
// A2-4：组账户（/config/groups.json）——与账户表同层的**纯用户态**消费。
// ---------------------------------------------------------------------------

/// 一条组记录（/config/groups.json 的 groups[] 元素）。
struct GroupRecord {
    name: String,
    gid: u32,
    members: Vec<String>,
}

/// 解析 /config/groups.json 字节流为组列表。
///
/// schema：{"groups":[{"name":"dev","gid":2000,"members":["alice","bob"]}, ...]}。
/// **畸形条目如实跳过**（缺 name/gid、名字非法），不 panic、不补默认值（S09）。
/// 与 libc::pwd::parse_groups 同 schema（同一份用户态格式，两处独立实现——
/// libc 不得依赖 userd 的私有代码）。
fn parse_groups(bytes: &[u8]) -> Vec<GroupRecord> {
    let mut out = Vec::new();
    let Ok(text) = core::str::from_utf8(bytes) else {
        log(b"groups.json not UTF-8; skipped");
        return out;
    };
    let mut p = libsys::json::JsonParser::new(text);
    let Ok(parsed) = p.parse() else {
        log(b"groups.json malformed JSON; skipped");
        return out;
    };
    if let libsys::json::JsonValue::Object(fields) = parsed {
        for (k, v) in fields {
            if k != "groups" {
                continue;
            }
            if let libsys::json::JsonValue::Array(items) = v {
                for it in items {
                    if let libsys::json::JsonValue::Object(obj) = it {
                        let mut name = String::from("");
                        let mut gid: Option<u32> = None;
                        let mut members: Vec<String> = Vec::new();
                        for (fk, fv) in obj {
                            match fk.as_str() {
                                "name" => {
                                    if let libsys::json::JsonValue::String(s) = fv {
                                        name = s;
                                    }
                                }
                                "gid" => {
                                    if let libsys::json::JsonValue::Number(n) = fv {
                                        gid = n.parse::<u32>().ok();
                                    }
                                }
                                "members" => {
                                    if let libsys::json::JsonValue::Array(ms) = fv {
                                        for m in ms {
                                            if let libsys::json::JsonValue::String(s) = m {
                                                members.push(s);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        let bad = name.is_empty()
                            || name == "."
                            || name == ".."
                            || name.contains('/');
                        if bad {
                            log(b"group with empty/invalid name; skipped");
                            continue;
                        }
                        if let Some(g) = gid {
                            out.push(GroupRecord { name, gid: g, members });
                        } else {
                            logf(format_args!("group {} missing gid; skipped", name));
                        }
                    }
                }
            }
        }
    }
    out
}

/// 构造 /groups/<name> 投影内容（A2-4：组表在文件系统中的**可见投影**）。
///
/// **为何要有投影**：`groups.json` 是唯一权威源（S13），但用户态程序按名查组需读
/// 一个 JSON 文件并解析——投影把「已解析的结论」固化为稳定路径，使 shell 与
/// 测试无需重复解析逻辑即可核对 userd 的消费结果。投影内容与源**同源**，
/// 不含源里没有的信息（不编造成员）。
fn group_json(rec: &GroupRecord) -> String {
    let mut target = libsys::json::VecTarget::new();
    let mut writer = libsys::json::JsonWriter::new(&mut target);
    if let Ok(mut obj) = writer.start_object() {
        let _ = obj.field_str("name", &rec.name);
        let _ = obj.field_u64("gid", rec.gid as u64);
        let _ = obj.field_u64("member_count", rec.members.len() as u64);
        // 成员名数组：经 JsonArray 逐个写入（转义由 writer 负责）。
        // 独立子 writer 生成该数组的 JSON 文本，再作为 field_raw 嵌入——
        // 避免在同一 writer 上嵌套借用（JsonObject 已独占 &mut target）。
        // 空列表如实写 []，不省略该字段（消费方无需区分"没有成员"与"字段缺失"）。
        let members_json = {
            let mut sub = libsys::json::VecTarget::new();
            {
                let mut w = libsys::json::JsonWriter::new(&mut sub);
                if let Ok(mut arr) = w.start_array() {
                    for m in &rec.members {
                        let _ = arr.push_str(m);
                    }
                    let _ = arr.end();
                }
            }
            sub.into_string().unwrap_or_else(|_| String::from("[]"))
        };
        let _ = obj.field_raw("members", &members_json);
        let _ = obj.end();
    }
    target.into_string().unwrap_or_else(|_| String::from("{}"))
}

/// 同步一组：确保 /groups/<name> 投影存在且内容最新。幂等（内容相同不重写）。
fn ensure_group_dir(rec: &GroupRecord) {
    let _ = mkdir("/groups", Permissions::all());
    let mut path = String::from("/groups/");
    path.push_str(&rec.name);
    let want = group_json(rec);
    let stale = match read_file(&path) {
        Some(cur) => cur != want.as_bytes(),
        None => true,
    };
    if stale {
        if let Err(e) = write_file(&path, want.as_bytes()) {
            logf(format_args!("write {} failed: {:?}", path, e));
        }
    }
    logf(format_args!("group ready: {} (gid {}, {} member(s))", rec.name, rec.gid, rec.members.len()));
}

/// 同步组表：读 /config/groups.json → 逐组建投影。
/// **表缺失如实记录并等下一轮**（不伪造空组表，不退出——表可能稍后出现）。
/// 返回 None 表示表暂不可读（调用方据此区分"没有组"与"还没读到组表"）。
fn reconcile_groups() -> Option<usize> {
    let bytes = match read_file("/config/groups.json") {
        Some(b) => b,
        None => {
            log(b"no /config/groups.json yet; waiting");
            return None;
        }
    };
    let groups = parse_groups(&bytes);
    if groups.is_empty() {
        log(b"groups.json empty/malformed; nothing to do");
        return Some(0);
    }
    for g in &groups {
        ensure_group_dir(g);
    }
    Some(groups.len())
}

#[unsafe(no_mangle)]
pub extern "C" fn user_main(_argc: isize, _argv: *const *const u8) -> i32 {
    log(b"userd started (ADR-040 section 2.8)");
    loop {
        reconcile();
        let _group_count = reconcile_groups();
        // 10s 对账周期：账户表是低频变更的管理面数据；sleep 挂起让出 CPU，
        // 不忙转（与 volumed 对账循环同族但更缓）。
        let _ = sleep(10_000_000_000);
    }
}
