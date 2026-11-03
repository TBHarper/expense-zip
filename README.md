# expense-zip

Zips the files in the current folder into `<Last><First>Expenses<YYYY><MM>.zip`, written to that same folder.

- `YYYY` is the parent folder's name, e.g. `2026`.
- `MM` is the first two characters of the current folder's name, e.g. `03` from `03 March`.

## Build

Your name is baked in at compile time from `FIRST_NAME` and `LAST_NAME`. Set them once in `.cargo/config.toml`, which is git-ignored so it stays local:

```toml
[env]
FIRST_NAME = "Jane"
LAST_NAME = "Doe"
```

Then run `cargo build --release`. Variables set in the shell override the file. The binary is `target\release\expense-zip.exe`.

## Usage

```powershell
cd "C:\Expenses\2026\03 March"
expense-zip          # -> DoeJaneExpenses202603.zip
```

| Flag | Meaning |
| --- | --- |
| `-r`, `--recurse` | Also include files in subfolders (default: top level only) |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## Notes

- Uses 7-Zip if found: first `7z` on PATH, then `C:\Program Files\7-Zip\7z.exe`, then `C:\Program Files (x86)\7-Zip\7z.exe`. Otherwise it uses a built-in zip writer, which stamps every file with the current time instead of its own.
- An existing zip with the same name is replaced.
- The program skips its own `.exe` if it's in the folder.
