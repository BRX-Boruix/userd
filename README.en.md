# userd

BORUIX's account daemon: syncs the account tables into home directories and identity files on the filesystem.

[简体中文](README.md)

Started by the system init process at boot; runs for the lifetime of the system.

## What it does

- Reads `/config/users.json` and creates `/users/<name>` for each account, owned by the account's uid and gid with mode 0700
- Maintains an identity file in each home directory recording uid, gid and name
- Reads `/config/groups.json` and writes a membership projection for each group at `/groups/<name>`
- Re-reads both tables every 10 seconds, creating newly added accounts and groups

## Behaviour

- If an account table is missing it waits, reporting once; sync starts automatically once the table appears
- Removing an account from a table never deletes its home directory; the data stays
- Entries with an empty name, the name `.` or `..`, or a `/` in the name are skipped and logged; so are entries missing uid or gid
- Home directories and projections are written only when their content changes

## Known limitations

- No tools to create or delete accounts; the tables are the single entry point
- Cleaning up data after an account is removed belongs to admin tools, not this daemon

## Building

```bash
cargo build --release
```

## Repository layout

```
userd/
├── Cargo.toml    # package manifest
├── build.rs      # injects the linker script
├── linker.ld     # user-space segment layout
└── src/
    └── main.rs   # table parsing, home sync, reconcile loop
```

## Related projects

- [`libsys`](https://github.com/BRX-Boruix/libsys) — user-space system call wrappers
- [`init`](https://github.com/BRX-Boruix/init) — starts this daemon
- [`login`](https://github.com/BRX-Boruix/login) — the login program
- [`libc`](https://github.com/BRX-Boruix/libc) — look-up-by-name interfaces for users and groups

## License

MIT License, copyright Yang Borui. See [LICENSE](LICENSE).
