# userd

BORUIX's **account daemon**: it turns the users listed in the account table into home directories that actually exist on the filesystem.

[简体中文](README.md)

## What it does

```
[userd] users.json loaded: 3 account(s)
[userd] /users/alice: created (uid=1000 gid=1000 mode=0700)
```

| Task | Details |
| --- | --- |
| Create the home directory | Creates `/users/<username>` for each account |
| Set ownership | Owner set to that user, permissions to "owner only" |
| Place an identity projection | Writes self-describing identity information (user ID, group ID, username) inside the directory |

The third is a **read-only projection** — placed in the user's own directory, readable by anyone but not written by the user.

## The kernel does not parse the account table

The account table is an **ordinary file**; the kernel neither parses it nor knows the concept of a "user". Creating directories and setting permissions all happen in user space with ordinary file operations — **no new system call was added for any of it**.

The value is **replaceability**: what account data looks like, where it lives, and its format are all user-space matters. Changing the account scheme requires no kernel change; the kernel provides generic file and permission machinery and stays out of account policy.

## Running repeatedly causes no harm

The process re-checks the account table periodically, which demands every operation be **idempotent**:

| Situation | Handling |
| --- | --- |
| The home directory already exists | Fine, carry on |
| The identity information is unchanged | **Do not rewrite** |

The second matters especially. Rewriting blindly on every pass produces endless pointless writes — each going through the filesystem, updating metadata, and producing log lines. **Deciding whether the content really changed before writing** drops the busywork and leaves the log showing only genuine changes.

## What happens when an account is removed from the table

**The home directory is not deleted.**

That is a deliberate decision for data safety: **"delete the account" and "delete the data" are not the same thing**. When an administrator removes a user from the table, the intent is usually "this person can no longer log in", not "destroy everything this person owned".

Were the two implemented as one, the result would be irreversible — a single mistaken operation permanently destroys an entire user's data.

So the process only **adds what is missing**: new accounts get their home directories built, removed accounts are **left untouched**. Cleaning up data belongs to dedicated administrative tools, not to something an automatic daemon does in passing.

## When the account table is abnormal

| Situation | Behaviour |
| --- | --- |
| The table is absent | Recorded honestly, then it stays resident and waits |
| The file is not valid text, or the JSON is malformed | Recorded honestly, skipped |
| One entry lacks a field or has an invalid number | **That entry alone is skipped**; the rest proceed |

It **never fabricates an empty account to fill the gap**. The last row deserves a note too: the account table is a user-space file and its **contents are not trusted**, so a malformed entry should neither crash the program nor disturb other valid accounts.

## A failed start is not fatal

The process is started by the system init process. **If it fails to start the system is still usable** — there are simply no home directories. The account service is a convenience, not a prerequisite for survival: it avoids the murky state of "half a system works".

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
