# userd

BORUIX 的账户守护进程：把账户表同步为文件系统中的家目录与身份文件。

[English](README.en.md)

由系统初始化进程在启动时拉起，之后常驻运行。

## 它做什么

- 读取 `/config/users.json`，为每个账户创建 `/users/<名字>`，属主设为账户的 uid 与 gid，权限 0700
- 在每个家目录维护身份文件 `identity`，内容为 uid、gid 与名字
- 读取 `/config/groups.json`，为每个组在 `/groups/<名字>` 写入成员投影
- 每 10 秒重读两张表，新增的账户与组自动补建

## 行为约定

- 账户表不存在时驻留等待，只提示一次；表出现后自动开始
- 从表中删除的账户不删除其家目录，数据保留
- 名字为空、为 `.` 或 `..` 或含 `/` 的条目跳过并记录；缺 uid 或 gid 的条目同样跳过
- 家目录与投影只在内容变化时写入

## 已知限制

- 不提供创建或删除账户的工具，账户表是唯一入口
- 删除账户后的数据清理归管理员工具，不在本程序职责内

## 构建

```bash
cargo build --release
```

## 文件结构

```
userd/
├── Cargo.toml    # 包定义
├── build.rs      # 注入链接脚本
├── linker.ld     # 用户态段布局
└── src/
    └── main.rs   # 表解析、家目录同步与对账循环
```

## 相关项目

- [`libsys`](https://github.com/BRX-Boruix/libsys) —— 用户态系统调用封装
- [`init`](https://github.com/BRX-Boruix/init) —— 拉起本程序
- [`login`](https://github.com/BRX-Boruix/login) —— 登录认证程序
- [`libc`](https://github.com/BRX-Boruix/libc) —— 提供按名查询用户与组的接口

## 许可

MIT License，版权归 Yang Borui 所有。详见 [LICENSE](LICENSE)。
