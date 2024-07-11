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
$ chrommydoggymommy run -i 100 -p 1000 -c chrommy.ckpt programs/program.py
... progress output ...
⠿ programs/program.py Completed in 107507284 ms     100
```
Note that the time output is the total amount of compute seconds. So, if the program ran four times for 8 ms in
parallel, the time taken would be 32 ms.

Work with checkpoint files:
```sh
$ chrommydoggymommy checkpoint summary chrommy.ckpt
Checkpoint file chrommy3.ckpt created at Fri 12 July 2024 00:06:
    ┗  programs/myamazingalgorithm.py 4a250102:
       Port counts computed: 1000 (10 runs, avg. swaps 909816.800)
    ┗  programs/alg2.py 05b6d900:
       Port counts computed: 1000 (10 runs, avg. swaps 898912.600)
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

## Todo
- [x] Produce checkpoint files
- [ ] Incrementally update checkpoint files
- [ ] Work with checkpoint files
  - [x] Merge checkpoint files
  - [ ] Dump contents of checkpoint files to CSV files
- [ ] Implement alternate runners