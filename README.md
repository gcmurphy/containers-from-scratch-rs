## containers-from-scratch - Rust edition.

#### About

Liz Rice gives a most excellent talk about Linux namespaces in which she builds
a container from the ground up. This is a rust port of her [example code](https://github.com/lizrice/containers-from-scratch).
Like the original there is still a lot that can be added to further isolate the process. I might add the rootless
container part at a later stage.

#### Usage

`sudo cargo run /bin/sh`

##### Caveats

- *Requires root permissions*
- *`$HOME/rootfs-x86_64` will be the basis of the chroot*

