# userd

**简体中文** | [English](#english)

BORUIX 的**账户守护进程**——把账户表里登记的用户，变成文件系统里真实存在的家目录。

```
[userd] users.json loaded: 3 account(s)
[userd] /users/alice: created (uid=1000 gid=1000 mode=0700)
```

---

## 它做什么

系统里有一个**账户表**（一份普通文件），记录有哪些用户、各自的用户 ID 与组 ID。

`userd` 负责把这些登记项**落实成实际的目录**：为每个账户创建家目录、把目录的所有者设成该用户、
权限设成"仅本人可访问"，并在里面放一份身份信息。

没有它，账户表就只是一份描述——用户登录后没有属于自己的目录。

## 一个关键的设计选择：内核不解析账户表

这个程序的存在本身来自一个设计决定：**账户表是普通文件，内核不去解析它，也不认识"用户"这个概念**。

解析账户、创建目录、设置权限，全部在用户态用**普通的文件操作**完成——**没有为此新增任何系统调用**。

这样做的价值在于**可替换性**：账户数据长什么样、放在哪里、用什么格式，都是用户态的事。换个
账户方案不需要动内核。内核只提供通用的文件与权限机制，不掺和具体的账户策略。

## 它做三件事

| 工作 | 内容 |
| --- | --- |
| **建家目录** | 为每个账户创建 `/users/<用户名>` |
| **设归属** | 把目录所有者设为该用户，权限设为"仅本人" |
| **放身份投影** | 在目录内写入一份身份信息（用户 ID、组 ID、用户名） |

第三项是一份**只读投影**——把账户信息以一种自解释的格式放在用户自己的目录里，谁都能看懂，
但不由用户修改。

## 反复执行也不会出问题

这个程序会**周期性地重新检查**账户表，把变化同步过来。这就要求它的每一项操作都是**幂等**的：

| 情况 | 处理 |
| --- | --- |
| 家目录已存在 | 正常，继续 |
| 身份信息内容没变 | **不重写** |

第二条尤其重要。如果每次巡检都无脑重写一遍身份文件，就会在系统里制造持续的、毫无意义的写入
——每一次写入都要经过文件系统、更新元数据、产生日志。**判断"内容是否真的变了"再决定写不写**，
既省掉了无用功，也让日志里只出现真正的变化。

## 表里删掉一个账户会怎样

**家目录不会被删除。**

这是一个刻意的、出于数据安全的决定："**删除账户"和"删除数据"不是一回事**。管理员把一个用户
从账户表里移除，通常意图是"这个人不能再登录了"，而不是"把这个人的所有文件都销毁"。

这两件事如果被实现成同一件事，后果是不可逆的——一次误操作就永久删除了一整个用户的数据。

所以这个守护进程只做**增量补齐**：表里新增的账户，补建家目录；表里移除的账户，**保持原样**。
清理数据属于专门的管理工具，不应该由一个自动巡检的守护进程顺手做掉。

## 账户表不存在或内容有问题时会怎样

程序处理得**如实且克制**：

| 情况 | 行为 |
| --- | --- |
| 账户表不存在 | 如实记录，继续驻留等待 |
| 文件不是合法文本 | 如实记录，跳过 |
| 内容是格式错误的 JSON | 如实记录，跳过 |
| 某一条记录缺字段或数值非法 | **只跳过这一条**，其余正常处理 |

**绝不伪造一个空账户来充数**——如果表读不到，就是读不到，不会凭空造出用户来。

最后一条也值得说明：账户表是用户态文件，**内容不可信**。一条畸形记录不应该让整个程序崩溃，
也不应该影响其他正常账户的处理。所以程序逐条容错，遇到坏的跳过并记录，继续往下走。

## 启动失败不是致命的

这个程序由系统初始化进程拉起。**如果它启动失败，系统仍然可用**——只是没有家目录而已。

这是一个有意的降级设计：账户服务是"锦上添花"，不是"没有它就活不了"。不制造"半个系统能用"
这种说不清的状态。

## 运行方式

由系统初始化进程在启动时拉起，之后常驻运行，周期性巡检账户表。

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
    └── main.rs   # 账户同步与巡检循环
```

## 相关项目

- [`login`](https://github.com/BRX-Boruix/login) —— 登录认证，使用账户信息
- [`init`](https://github.com/BRX-Boruix/init) —— 拉起本守护进程
- [`libsys`](https://github.com/BRX-Boruix/libsys) —— 用户态系统调用封装
- [`pwde2e`](https://github.com/BRX-Boruix/pwde2e) —— 账户查询接口的验收程序

## 许可

MIT License，版权归 Yang Borui 所有。详见 [LICENSE](LICENSE)。

---

# English

[简体中文](#userd) | **English**

BORUIX's **account daemon** — it turns the users registered in the account table into home
directories that actually exist on the filesystem.

```
[userd] users.json loaded: 3 account(s)
[userd] /users/alice: created (uid=1000 gid=1000 mode=0700)
```

---

## What it does

The system has an **account table** (an ordinary file) listing which users exist and their user and
group IDs.

`userd` turns those entries into **real directories**: it creates a home directory for each account,
sets the directory's owner to that user, sets its permissions to "owner only", and places identity
information inside.

Without it the account table would be a description only — a user could log in and find no directory
of their own.

## A key design choice: the kernel does not parse the account table

This program exists because of a design decision: **the account table is an ordinary file, and the
kernel neither parses it nor knows the concept of a "user"**.

Parsing accounts, creating directories, and setting permissions all happen in user space using
**ordinary file operations** — **no new system call was added for any of it**.

The value is **replaceability**: what account data looks like, where it lives, and what format it
takes are all user-space matters. Changing the account scheme requires no kernel change. The kernel
provides generic file and permission machinery and stays out of account policy.

## The three things it does

| Task | Contents |
| --- | --- |
| **Create the home directory** | Creates `/users/<username>` for each account |
| **Set ownership** | Sets the directory owner to that user, permissions to "owner only" |
| **Place an identity projection** | Writes identity information (user ID, group ID, username) inside the directory |

The third is a **read-only projection** — account information in a self-describing format, placed in
the user's own directory, readable by anyone but not written by the user.

## Running it repeatedly causes no harm

The program **periodically re-checks** the account table and syncs changes over. That demands every
operation be **idempotent**:

| Situation | Handling |
| --- | --- |
| The home directory already exists | Fine, carry on |
| The identity information is unchanged | **Do not rewrite** |

The second matters especially. Rewriting the identity file blindly on every pass would generate
endless, pointless writes through the system — each one going through the filesystem, updating
metadata, and producing log lines. **Deciding whether the content really changed before writing**
drops the busywork and leaves the log showing only genuine changes.

## What happens when an account is removed from the table

**The home directory is not deleted.**

That is a deliberate decision for data safety: **"delete the account" and "delete the data" are not
the same thing**. When an administrator removes a user from the account table, the intent is usually
"this person can no longer log in", not "destroy everything this person owned".

Were the two implemented as one, the result would be irreversible — a single mistaken operation
permanently destroys an entire user's data.

So the daemon only **adds what is missing**: newly listed accounts get their home directories built;
removed accounts are **left untouched**. Cleaning up data belongs to dedicated administrative tools,
not to something an automatic daemon does in passing.

## What happens when the account table is missing or malformed

The program is **honest and restrained**:

| Situation | Behaviour |
| --- | --- |
| The account table is absent | Recorded honestly, then it stays resident and waits |
| The file is not valid text | Recorded honestly, skipped |
| The content is malformed JSON | Recorded honestly, skipped |
| One entry lacks a field or has an invalid number | **That entry alone is skipped**; the rest proceed |

It **never fabricates an empty account to fill the gap** — if the table cannot be read, it cannot be
read, and no user is invented out of thin air.

The last row deserves a note too: the account table is a user-space file and its **contents are not
trusted**. A single malformed entry should neither crash the program nor disturb the handling of
other valid accounts. So the program tolerates errors entry by entry — skipping and recording the bad
one, then carrying on.

## A failed start is not fatal

The program is started by the system init process. **If it fails to start, the system is still
usable** — there are simply no home directories.

That is a deliberate degradation: the account service is a convenience, not a prerequisite for
survival. It avoids the murky state of "half a system works".

## How it runs

Started by the system init process at boot, then resident, periodically reconciling the account
table.

## Building

```bash
cargo build --release
```

## Layout

```
userd/
├── Cargo.toml    # package definition
├── build.rs      # injects the linker script
├── linker.ld     # user-space section layout
└── src/
    └── main.rs   # account sync and the reconcile loop
```

## Related projects

- [`login`](https://github.com/BRX-Boruix/login) — login authentication, which uses account information
- [`init`](https://github.com/BRX-Boruix/init) — starts this daemon
- [`libsys`](https://github.com/BRX-Boruix/libsys) — the user-space syscall wrapper
- [`pwde2e`](https://github.com/BRX-Boruix/pwde2e) — acceptance for the account lookup interfaces

## License

MIT License, copyright Yang Borui. See [LICENSE](LICENSE).
