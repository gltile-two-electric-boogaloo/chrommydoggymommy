# chrommydoggymommy

Overengineered test runner

## Platform support

Linux only. May theoretically run on other platforms, haven't tested.

## Get started
```sh
$ git clone https://github.com/gltile-two-electric-boogaloo/chrommydoggymommy
$ cd chrommydoggymommy
$ cargo install --path .
$ chrommydoggymommy --help
```

Run a program 100 times on 1000 ports:
```shell
$ chrommydoggymommy -i 100 -p 1000 -c chrommy.ckpt programs/program.py
... progress output ...
⠿ programs/program.py Completed in 107507284 ms     100
```

Work with checkpoint files:
```sh
$ chrommydoggymommy checkpoint merge chrommy.ckpt chrommy2.ckpt
$ chrommydoggymommy checkpoint dump chrommy.ckpt chrommy.csv
```

## Safety

This program makes two assumptions:

- Data flowing through pipes between it and its child processes will not be tampered with.
- Files on disk will not be tampered with.
- Files on disk will be read by the same machine that it was written by.

If these assumptions cannot be held, undefined behaviour may arise.
**Do not load checkpoint files from other people.**