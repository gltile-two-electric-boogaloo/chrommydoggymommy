# chrommydoggymommy

Overengineered test runner

## Platform support

Linux only. May theoretically run on other platforms, haven't tested.

## Get started
```sh
git clone https://github.com/gltile-two-electric-boogaloo/chrommydoggymommy
cd chrommydoggymommy
cargo install --path .
chrommydoggymommy --help
```

## Safety

This program makes two assumptions:

- Data flowing through pipes between it and its child processes will not be tampered with.
- Files on disk will not be tampered with.
- Files on disk will be read by the same machine that it was written by.

If these assumptions cannot be held, undefined behaviour may arise.
**Do not load checkpoint files from other people.**