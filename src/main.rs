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
const DEFAULT_CRYFS_BIN: &str = "/home/user1/.local/opt/cryfs-1.0.3/bin/cryfs";
const DEFAULT_CRYFS_UNMOUNT_BIN: &str = "/home/user1/.local/opt/cryfs-1.0.3/bin/cryfs-unmount";

#[derive(Debug, Parser)]
#[command(
    name = "cryfs-gpg",
    about = "Monta um sistema CryFS protegido por GnuPG"
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
    std::fs::create_dir_all(&config_dir).context("criando diretório CFG")?;

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

    let config = tempfile::NamedTempFile::new().context("criando configuração temporária")?;
    decrypt_to_path(&encrypted_config, config.path(), &options.recipients)?;
    mount(&options, config.path(), &passphrase)?;
    Ok(())
}

fn generate_passphrase() -> Result<String> {
    let username = std::env::var("USER").context("variável USER não definida")?;
    let seed = tempfile::NamedTempFile::new()?
        .path()
        .to_string_lossy()
        .into_owned();
    let salt = CString::new(format!("$6${username}")).context("salt inválido")?;
    let password = CString::new(seed).context("senha inválida")?;
    let result = unsafe { crypt(password.as_ptr(), salt.as_ptr()) };
    if result.is_null() {
        bail!("não foi possível gerar a senha SHA-512");
    }
    Ok(unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned())
}

fn encrypt_file(data: &str, destination: &Path, recipients: &[String]) -> Result<()> {
    if recipients.is_empty() {
        bail!("informe ao menos um destinatário GPG com --recipient");
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
        .context("executando gpg para criptografia")?;
    child.stdin.take().unwrap().write_all(data.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "gpg falhou ao criptografar: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    truncate(destination, DEFAULT_BLOCK_SIZE)?;
    Ok(())
}

fn decrypt_file(source: &Path, recipients: &[String]) -> Result<String> {
    let output = gpg_decrypt(source, recipients)?;
    let output = output.split(|byte| *byte == 0).next().unwrap_or_default();
    String::from_utf8(output.to_vec()).context("arquivo descriptografado não é UTF-8")
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
        .context("executando gpg para descriptografia")?;
    // Os arquivos do formato original são preenchidos/truncados em 32 KiB.
    // O GPG pode emitir a mensagem "invalid packet" pelos bytes finais, mas
    // ainda entregar corretamente o conteúdo descriptografado em stdout.
    if !output.status.success() && output.stdout.is_empty() {
        bail!(
            "gpg falhou ao descriptografar: {}",
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
    let mut input = child.stdin.take().context("abrindo entrada do cryfs")?;
    input.write_all(format!("{passphrase}\n{passphrase}\n").as_bytes())?;
    drop(input);
    let status = child.wait().context("aguardando cryfs")?;
    if !status.success() && status.code() != Some(2) {
        bail!("cryfs terminou durante a criação da configuração com {status}");
    }
    encrypt_file_from_path(&temporary, encrypted_config, &options.recipients)?;
    Ok(())
}

fn encrypt_file_from_path(source: &Path, destination: &Path, recipients: &[String]) -> Result<()> {
    if recipients.is_empty() {
        bail!("informe ao menos um destinatário GPG com --recipient");
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
        .context("executando gpg para criptografar a configuração")?;
    if !output.status.success() {
        bail!(
            "gpg falhou ao criptografar a configuração: {}",
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
        bail!("cryfs terminou com {status}");
    }
    Ok(())
}

fn validate(options: &Options) -> Result<()> {
    let basedir = options.basedir.as_ref().context("basedir é obrigatório")?;
    if !basedir.is_dir() {
        bail!(
            "basedir não existe ou não é diretório: {}",
            basedir.display()
        );
    }
    if !options.mountdir.is_dir() {
        bail!(
            "mountdir não existe ou não é diretório: {}",
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
        .context("verificando se o diretório está montado")?;
    if !mounted.success() {
        return Ok(());
    }

    let status = Command::new(cryfs_unmount_bin())
        .arg(mountdir)
        .status()
        .context("executando cryfs-unmount")?;
    if !status.success() {
        bail!("cryfs-unmount terminou com {status}");
    }
    Ok(())
}

fn require_command(path: &str) -> Result<()> {
    let status = Command::new(path)
        .arg("--version")
        .status()
        .with_context(|| format!("{path} não está instalado ou não pode ser executado"))?;
    if !status.success() {
        bail!("{path} não respondeu corretamente a --version");
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
