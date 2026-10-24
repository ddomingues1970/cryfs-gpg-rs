# cryfs-gpg-rs

Aplicação Rust para montar sistemas de arquivos CryFS protegendo a configuração e a senha do CryFS com GnuPG.

O projeto é a migração do `cryfs-gpg` original, escrito em C. Ele mantém a mesma ideia de segurança: a configuração e a senha usadas pelo CryFS são criptografadas com uma chave pública GPG e só são recuperadas durante a montagem, usando a chave privada correspondente.

## Principais melhorias

- Migração de C para Rust, com verificação de tipos e gerenciamento seguro de memória.
- Uso explícito do CryFS 1.0.3 por padrão.
- Caminho do CryFS configurável por `CRYFS_BIN`.
- Caminho do `cryfs-unmount` configurável por `CRYFS_UNMOUNT_BIN`.
- Tamanho de bloco configurável com `--blocksize`.
- Desmontagem integrada com `--unmount`.
- Desmontagem idempotente: não falha quando o diretório já está desmontado.
- Interface de linha de comando documentada automaticamente pelo `clap`.
- Tratamento de erros mais claro e ausência de manipulação manual de memória da aplicação.

## Requisitos

- Linux com suporte a FUSE.
- CryFS 1.0.3.
- GnuPG com uma chave pública para o destinatário e a chave privada correspondente para montagem posterior.
- Rust e Cargo para compilar o projeto.

## Compilação

```bash
cargo build --release
```

O executável será criado em:

```text
target/release/cryfs-gpg
```

Exemplo de instalação com um nome separado da versão C:

```bash
sudo install -m 0755 target/release/cryfs-gpg /usr/bin/cryfs-gpg-r
```

## Configuração do CryFS

Por padrão, a aplicação usa:

```text
/home/user1/.local/opt/cryfs-1.0.3/bin/cryfs
/home/user1/.local/opt/cryfs-1.0.3/bin/cryfs-unmount
```

Para usar outro local:

```bash
export CRYFS_BIN=/caminho/para/cryfs
export CRYFS_UNMOUNT_BIN=/caminho/para/cryfs-unmount
```

## Criar e montar um novo volume

```bash
mkdir -p ~/dropbox/basedir ~/dropbox/mountdir

cryfs-gpg-r \
  --basedir ~/dropbox/basedir \
  --mountdir ~/dropbox/mountdir \
  --recipient seu-email@example.com
```

O parâmetro `--recipient` é necessário na primeira criação, pois a configuração e a senha do CryFS serão criptografadas para essa chave GPG.

Depois da criação inicial, a montagem pode ser feita com:

```bash
cryfs-gpg-r \
  --basedir ~/dropbox/basedir \
  --mountdir ~/dropbox/mountdir
```

## Desmontar

```bash
cryfs-gpg-r --unmount --mountdir ~/dropbox/mountdir
```

O comando também pode ser executado quando o diretório já estiver desmontado.

## Uso com armazenamento em nuvem

Os arquivos dentro de `basedir` permanecem criptografados pelo CryFS e podem ser sincronizados com ferramentas como o rclone:

```bash
rclone sync ~/dropbox/basedir remote:cryfs-backup
```

Os arquivos devem ser acessados normalmente através de `mountdir`. Não sincronize a pasta montada; sincronize somente a pasta criptografada `basedir`.

## Migração da versão C

Antes de migrar, faça backup dos dados acessados pelo volume antigo e preserve a base criptografada original. A migração recomendada é:

1. Montar o volume com a versão C.
2. Copiar os dados descriptografados para dois locais de backup.
3. Desmontar o volume antigo.
4. Preservar a base criptografada antiga em um diretório separado.
5. Criar uma nova base vazia para o CryFS 1.0.3.
6. Montar a nova base com `cryfs-gpg-r`.
7. Restaurar os dados dos backups para o novo volume.
8. Validar leitura, gravação e desmontagem antes de remover qualquer backup.

Não apague a base antiga até confirmar que a nova montagem e os backups estão íntegros.

## Segurança

A aplicação não armazena a senha do CryFS em texto claro na base. A configuração e a senha são criptografadas com GnuPG e recuperadas somente durante a montagem.

A segurança final depende da proteção da chave privada GPG, da senha dessa chave, das permissões do sistema e dos backups. A aplicação não substitui uma política de backup ou o gerenciamento seguro das chaves.

## Status

O fluxo principal foi validado com CryFS 1.0.3:

- criação de volume;
- montagem;
- gravação e leitura de arquivos;
- desmontagem integrada;
- restauração dos dados a partir de backup;
- execução com blocksize de 32768 bytes.

## Licença

Consulte o arquivo `LICENSE` da versão original para as condições de distribuição do projeto.
