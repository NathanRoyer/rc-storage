# rc-storage

Web interface to mount and browse storage partitions.
It requires partitions to have a node in /dev.

### Usage

```sh
cargo install rc-storage
cd /mnt
sudo rc-storage
# visit http://localhost:8080/
```

### To do:

- allow specification of mount directory in command line arguments
- allow specification of address to bind the server to
- make interface mobile-friendly
- remove the requirement for root access to mount
- show filesystem type in partition list (low priority)

![parts.png](https://github.com/NathanRoyer/rc-storage/raw/main/parts.png)

![files.png](https://github.com/NathanRoyer/rc-storage/raw/main/files.png)
