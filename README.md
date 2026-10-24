# cryfs-gpg-rs

Rust application for mounting CryFS filesystems while protecting the CryFS configuration and password with GnuPG.

This project is the migration of the original C-based `cryfs-gpg` application. It preserves the same security model: the CryFS configuration and password are encrypted with a GPG public key and recovered only during mounting using the corresponding private key.

## Main improvements

- Migrated from C to Rust, with compile-time type checking and safer memory management.
- Explicit support for CryFS 1.0.3 by default.
- Configurable CryFS path through `CRYFS_BIN`.
- Configurable `cryfs-unmount` path through `CRYFS_UNMOUNT_BIN`.
- Configurable block size through `--blocksize`.
- Integrated unmount command through `--unmount`.
- Idempotent unmounting: it succeeds when the mount point is already unmounted.
- Command-line interface documented automatically by `clap`.
- Clearer error handling and no manual application-level memory management.

## Requirements

- Linux with FUSE support.
- CryFS 1.0.3.
- GnuPG with a public key for the recipient and the corresponding private key for later mounts.
- Rust and Cargo to build the project.

## Building

```bash
cargo build --release
```

The executable is created at `target/release/cryfs-gpg`.

Install it separately from the C version:

```bash
sudo install -m 0755 target/release/cryfs-gpg /usr/bin/cryfs-gpg-r
```

## CryFS configuration

By default, the application searches for these commands in `PATH`:

```text
cryfs
cryfs-unmount
```

To explicitly use CryFS 1.0.3 or another installation:

```bash
export CRYFS_BIN=/path/to/cryfs
export CRYFS_UNMOUNT_BIN=/path/to/cryfs-unmount
```

## Creating and mounting a new volume

```bash
mkdir -p ~/dropbox/basedir ~/dropbox/mountdir

cryfs-gpg-r \
  --basedir ~/dropbox/basedir \
  --mountdir ~/dropbox/mountdir \
  --recipient your-email@example.com
```

The `--recipient` option is required when creating a volume for the first time because the CryFS configuration and password are encrypted for that GPG key.

Mount an existing volume with:

```bash
cryfs-gpg-r \
  --basedir ~/dropbox/basedir \
  --mountdir ~/dropbox/mountdir
```

## Unmounting

```bash
cryfs-gpg-r --unmount --mountdir ~/dropbox/mountdir
```

The command also succeeds when the directory is already unmounted.

## Cloud storage

Files inside `basedir` remain encrypted by CryFS and can be synchronized with tools such as rclone:

```bash
rclone sync ~/dropbox/basedir remote:cryfs-backup
```

Access files through `mountdir`. Do not synchronize the mounted directory; synchronize only the encrypted `basedir` directory.

## Migration from the C version

Before migrating, back up the data accessed through the old volume and preserve the original encrypted base:

1. Mount the volume with the C version.
2. Copy the decrypted data to two independent backup locations.
3. Unmount the old volume.
4. Preserve the old encrypted base in a separate directory.
5. Create an empty base for CryFS 1.0.3.
6. Mount the new base with `cryfs-gpg-r`.
7. Restore the data from the backups into the new volume.
8. Validate reading, writing, and unmounting before removing any backup.

Do not delete the old base until the new mount and all backups have been verified.

## Security

The application does not store the CryFS password in plain text inside the encrypted base. The configuration and password are encrypted with GnuPG and recovered only during mounting.

Overall security depends on protecting the GPG private key, its passphrase, filesystem permissions, and backups. This application does not replace a backup policy or secure key management.

## Status

The main workflow has been validated with CryFS 1.0.3:

- volume creation;
- mounting;
- file writing and reading;
- integrated unmounting;
- restoring data from backup;
- operation with a 32768-byte block size.

## License

This project is licensed under the GNU General Public License, version 3 only
(`GPL-3.0-only`). See the `LICENSE` file for the complete terms.
