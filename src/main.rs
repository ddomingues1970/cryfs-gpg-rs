use anyhow::{bail, Context, Result};
use clap::Parser;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}

const CONFIG_DIR: &str = "CFG";
const CONFIG_FILE: &str = "EF95246C74537DDAFBFC508615B31";
const PASSPHRASE_FILE: &str = "EF95246C74537DDAFBFC508615B30";
const DEFAULT_BLOCK_SIZE: u64 = 32 * 1024;
const DEFAULT_CRYFS_BIN: &str = "cryfs";
const DEFAULT_CRYFS_UNMOUNT_BIN: &str = "cryfs-unmount";

#[derive(Debug, Parser)]
#[command(
    name = "cryfs-gpg",
    about = "Mount a CryFS filesystem protected by GnuPG"
)]
struct Options {
    #[arg(short = 'b', long = "basedir", required_unless_present = "unmount")]
    basedir: Option<PathBuf>,
    #[arg(short = 'm', long = "mountdir")]
    mountdir: PathBuf,
    #[arg(short = 'r', long = "recipient")]
    recipients: Vec<String>,
    #[arg(short = 's', long = "blocksize", default_value_t = DEFAULT_BLOCK_SIZE)]
    blocksize: u64,
    #[arg(short, long)]
    foreground: bool,
    #[arg(long)]
    unmount: bool,
}

fn main() -> Result<()> {
    let options = Options::parse();
    if options.unmount {
        return unmount(&options.mountdir);
    }
    validate(&options)?;

    let basedir = options.basedir.as_ref().unwrap();

    let config_dir = basedir.join(CONFIG_DIR);
    std::fs::create_dir_all(&config_dir).context("creating CFG directory")?;

    let encrypted_config = config_dir.join(CONFIG_FILE);
    let encrypted_passphrase = config_dir.join(PASSPHRASE_FILE);

    let passphrase = if encrypted_passphrase.exists() {
        decrypt_file(&encrypted_passphrase, &options.recipients)?
    } else {
        let passphrase = generate_passphrase()?;
        encrypt_file(&passphrase, &encrypted_passphrase, &options.recipients)?;
        passphrase
    };

    if !encrypted_config.exists() {
        create_config(&options, basedir, &config_dir, &encrypted_config, &passphrase)?;
        return Ok(());
    }

    let config = tempfile::NamedTempFile::new().context("creating temporary configuration")?;
    decrypt_to_path(&encrypted_config, config.path(), &options.recipients)?;
    mount(&options, config.path(), &passphrase)?;
    Ok(())
}

fn generate_passphrase() -> Result<String> {
    let username = std::env::var("USER").context("USER environment variable is not set")?;
    let seed = tempfile::NamedTempFile::new()?
        .path()
        .to_string_lossy()
        .into_owned();
    let salt = CString::new(format!("$6${username}")).context("salt inválido")?;
    let password = CString::new(seed).context("invalid password")?;
    let result = unsafe { crypt(password.as_ptr(), salt.as_ptr()) };
    if result.is_null() {
        bail!("could not generate the SHA-512 password");
    }
    Ok(unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned())
}

fn encrypt_file(data: &str, destination: &Path, recipients: &[String]) -> Result<()> {
    if recipients.is_empty() {
        bail!("provide at least one GPG recipient with --recipient");
    }
    let mut command = Command::new("gpg");
    command.args(["--batch", "--yes", "--trust-model", "always", "--encrypt"]);
    for recipient in recipients {
        command.args(["--recipient", recipient]);
    }
    command
        .arg("--output")
        .arg(destination)
        .stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .context("running gpg for encryption")?;
    child.stdin.take().unwrap().write_all(data.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "gpg encryption failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    truncate(destination, DEFAULT_BLOCK_SIZE)?;
    Ok(())
}

fn decrypt_file(source: &Path, recipients: &[String]) -> Result<String> {
    let output = gpg_decrypt(source, recipients)?;
    let output = output.split(|byte| *byte == 0).next().unwrap_or_default();
    String::from_utf8(output.to_vec()).context("decrypted file is not UTF-8")
}

fn decrypt_to_path(source: &Path, destination: &Path, recipients: &[String]) -> Result<()> {
    std::fs::write(destination, gpg_decrypt(source, recipients)?)?;
    Ok(())
}

fn gpg_decrypt(source: &Path, _recipients: &[String]) -> Result<Vec<u8>> {
    let output = Command::new("gpg")
        .args(["--batch", "--yes", "--decrypt"])
        .arg(source)
        .output()
        .context("running gpg for decryption")?;
    // Files in the original format are padded/truncated to 32 KiB.
    // GPG may report "invalid packet" for trailing bytes while still
    // correctly returning the decrypted content on stdout.
    if !output.status.success() && output.stdout.is_empty() {
        bail!(
            "gpg decryption failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn truncate(path: &Path, size: u64) -> Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(size)?;
    Ok(())
}

fn create_config(
    options: &Options,
    basedir: &Path,
    config_dir: &Path,
    encrypted_config: &Path,
    passphrase: &str,
) -> Result<()> {
    let temporary_dir = tempfile::tempdir_in(config_dir)?;
    let temporary = temporary_dir.path().join("cryfs-config");
    let mut child = Command::new(cryfs_bin())
        .arg("--config")
        .arg(&temporary)
        .arg("--blocksize")
        .arg(options.blocksize.to_string())
        .arg(basedir)
        .arg(&options.mountdir)
        .env("CRYFS_FRONTEND", "noninteractive")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("iniciando cryfs")?;
    let mut input = child.stdin.take().context("opening cryfs stdin")?;
    input.write_all(format!("{passphrase}\n{passphrase}\n").as_bytes())?;
    drop(input);
    let status = child.wait().context("waiting for cryfs")?;
    if !status.success() && status.code() != Some(2) {
        bail!("cryfs exited while creating the configuration with {status}");
    }
    encrypt_file_from_path(&temporary, encrypted_config, &options.recipients)?;
    Ok(())
}

fn encrypt_file_from_path(source: &Path, destination: &Path, recipients: &[String]) -> Result<()> {
    if recipients.is_empty() {
        bail!("provide at least one GPG recipient with --recipient");
    }
    let mut command = Command::new("gpg");
    command.args(["--batch", "--yes", "--trust-model", "always", "--encrypt"]);
    for recipient in recipients {
        command.args(["--recipient", recipient]);
    }
    let output = command
        .arg("--output")
        .arg(destination)
        .arg(source)
        .output()
        .context("running gpg to encrypt the configuration")?;
    if !output.status.success() {
        bail!(
            "gpg failed to encrypt the configuration: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    truncate(destination, DEFAULT_BLOCK_SIZE)
}

fn mount(options: &Options, config: &Path, passphrase: &str) -> Result<()> {
    let mut command = Command::new(cryfs_bin());
    command
        .args(["--config"])
        .arg(config)
        .arg(options.basedir.as_ref().unwrap())
        .arg(&options.mountdir);
    if options.foreground {
        command.arg("--foreground");
    }
    command
        .env("CRYFS_NO_UPDATE_CHECK", "true")
        .stdin(Stdio::piped());
    let mut child = command.spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{passphrase}\n").as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!("cryfs exited with {status}");
    }
    Ok(())
}

fn validate(options: &Options) -> Result<()> {
    let basedir = options.basedir.as_ref().context("basedir is required")?;
    if !basedir.is_dir() {
        bail!(
            "basedir does not exist or is not a directory: {}",
            basedir.display()
        );
    }
    if !options.mountdir.is_dir() {
        bail!(
            "mountdir does not exist or is not a directory: {}",
            options.mountdir.display()
        );
    }
    require_command(&cryfs_bin())?;
    require_command("gpg")?;
    Ok(())
}

fn unmount(mountdir: &Path) -> Result<()> {
    let mounted = Command::new("mountpoint")
        .arg("-q")
        .arg(mountdir)
        .status()
        .context("checking whether the directory is mounted")?;
    if !mounted.success() {
        return Ok(());
    }

    let status = Command::new(cryfs_unmount_bin())
        .arg(mountdir)
        .status()
        .context("running cryfs-unmount")?;
    if !status.success() {
        bail!("cryfs-unmount exited with {status}");
    }
    Ok(())
}

fn require_command(path: &str) -> Result<()> {
    let status = Command::new(path)
        .arg("--version")
        .status()
        .with_context(|| format!("{path} is not installed or cannot be executed"))?;
    if !status.success() {
        bail!("{path} did not respond correctly to --version");
    }
    Ok(())
}

fn cryfs_bin() -> String {
    std::env::var("CRYFS_BIN").unwrap_or_else(|_| DEFAULT_CRYFS_BIN.to_owned())
}

#[allow(dead_code)]
fn cryfs_unmount_bin() -> String {
    std::env::var("CRYFS_UNMOUNT_BIN")
        .unwrap_or_else(|_| DEFAULT_CRYFS_UNMOUNT_BIN.to_owned())
}

#[allow(dead_code)]
fn remove_trailing_slash(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().trim_end_matches('/'))
}
