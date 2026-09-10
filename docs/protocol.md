# Protocolo bt-file-transfer

Protocolo binário usado na conexão RFCOMM (Bluetooth Classic / SPP) entre
o `android-app` (cliente, sempre quem envia o arquivo) e o
`windows-server` (servidor, sempre quem recebe). É implementado de forma
independente nos dois lados:

- Rust: `windows-server/src/protocol.rs`
- Dart: `android-app/lib/protocol.dart`

As duas implementações precisam permanecer byte-a-byte compatíveis.
Qualquer mudança de formato precisa ser replicada nos dois arquivos.

## UUID do serviço RFCOMM

```
818711c5-3946-4523-b54b-20ac27970afe
```

Esse UUID é fixo e hardcoded em ambos os lados:

- `windows-server/src/server.rs` (constante `SERVICE_UUID`)
- `android-app/lib/file_transfer_service.dart` / documentado aqui para uso
  na descoberta do serviço SPP pelo endereço MAC do dispositivo pareado.

Se você for gerar um UUID próprio para o seu fork, gere um novo UUID v4
(`uuidgen` ou `python3 -c "import uuid; print(uuid.uuid4())"`) e atualize
os dois lados e este documento.

## Formato binário do frame

Todos os inteiros multi-byte são **little-endian**.

```
+----------+------------------+-----------------+-----------+
| tipo (1) | tamanho_lp (4)   | payload (N)     | crc32 (4) |
+----------+------------------+-----------------+-----------+
```

| Campo         | Tamanho  | Descrição                                            |
|---------------|----------|-------------------------------------------------------|
| `tipo`        | 1 byte   | Um dos 6 tipos de pacote (ver abaixo).                |
| `tamanho_lp`  | 4 bytes  | Tamanho em bytes do campo `payload`, `u32` LE.        |
| `payload`     | N bytes  | Conteúdo específico do tipo de pacote.                |
| `crc32`       | 4 bytes  | CRC-32 (IEEE 802.3/zlib, polinômio `0xEDB88320`) de `tipo + tamanho_lp + payload`, `u32` LE. |

O CRC32 cobre **tudo exceto o próprio campo de CRC**. Um receptor que
descobre um CRC inválido descarta o frame e responde com `NACK`.

Tamanho máximo de payload aceito: 1 MiB (proteção contra alocação
desenfreada por um frame corrompido). Tamanho de cada chunk de arquivo:
**4096 bytes** (o último chunk pode ser menor).

## Os 6 tipos de pacote

| Byte   | Tipo    | Direção típica      | Quando é usado                                                                 |
|--------|---------|---------------------|----------------------------------------------------------------------------------|
| `0x01` | `HELLO` | cliente → servidor, servidor → cliente | Primeiro pacote trocado na conexão. Cada lado envia/recebe um HELLO para confirmar que fala o mesmo protocolo antes de qualquer dado. |
| `0x02` | `META`  | cliente → servidor  | Enviado uma vez, logo após o handshake, com nome do arquivo, tamanho total e hash SHA-256 esperado. |
| `0x03` | `CHUNK` | cliente → servidor  | Um pedaço de até 4096 bytes do conteúdo do arquivo, identificado por um índice sequencial começando em 0. |
| `0x04` | `ACK`   | servidor → cliente  | Confirmação positiva: do META (índice implícito 0) ou de um CHUNK específico (pelo índice). |
| `0x05` | `NACK`  | servidor → cliente  | Confirmação negativa de um CHUNK (CRC inválido, fora de ordem, erro de E/S). O cliente deve reenviar o mesmo chunk. |
| `0x06` | `DONE`  | servidor → cliente  | Enviado depois que todos os chunks esperados (`file_size` bytes) foram recebidos. Carrega o resultado da verificação de integridade final (SHA-256 recalculado). |

## Formato dos payloads

### `HELLO`

```
+-----------+
| versao(1) |
+-----------+
```

1 byte com a versão do protocolo (atualmente `0x01`), para permitir
evolução futura sem quebrar clientes/servidores antigos.

### `META`

```
+------------------+------------------+---------------+----------------+
| nome_len (2, LE)  | nome (UTF-8, N)  | tamanho (8, LE)| sha256 (32)   |
+------------------+------------------+---------------+----------------+
```

- `nome_len`: tamanho em bytes (não em caracteres) do nome do arquivo, `u16` LE.
- `nome`: nome do arquivo, codificado em UTF-8 (sem caminho/diretório).
- `tamanho`: tamanho total do arquivo em bytes, `u64` LE.
- `sha256`: hash SHA-256 do arquivo completo, 32 bytes brutos.

### `CHUNK`

```
+-----------------+------------------+
| indice (4, LE)  | dados (ate 4096) |
+-----------------+------------------+
```

- `indice`: índice sequencial do chunk (`0`, `1`, `2`, ...), `u32` LE.
- `dados`: até 4096 bytes de conteúdo bruto do arquivo. O último chunk de
  uma transferência pode ter menos de 4096 bytes.

### `ACK`

```
+-----------------+
| indice (4, LE)  |
+-----------------+
```

Índice do pacote confirmado. Por convenção, o `ACK` do `META` usa índice
`0` (não há chunk de índice negativo).

### `NACK`

```
+-----------------+-----------+
| indice (4, LE)  | motivo(1) |
+-----------------+-----------+
```

`motivo`: `0` = CRC inválido, `1` = erro de E/S no servidor, `2` = chunk
fora de ordem, `255` = outro.

### `DONE`

```
+---------+
| ok (1)  |
+---------+
```

`1` = hash SHA-256 recalculado pelo servidor confere com o do `META`
(transferência íntegra); `0` = não confere (o servidor descarta o
arquivo recebido).

## Diagrama de sequência

```
Android (cliente)                          Windows (servidor)
      |                                            |
      |----------------- HELLO ------------------->|
      |<---------------- HELLO --------------------|
      |                                            |
      |----------------- META --------------------->|  (nome, tamanho, sha256)
      |<---------------- ACK(0) --------------------|
      |                                            |
      |----------------- CHUNK(0) ------------------>|
      |<---------------- ACK(0) --------------------|
      |----------------- CHUNK(1) ------------------>|
      |<---------------- NACK(1, crc) --------------|   (exemplo: chunk corrompido)
      |----------------- CHUNK(1) ------------------>|   (reenvio)
      |<---------------- ACK(1) --------------------|
      |                    ...                       |
      |----------------- CHUNK(N) ------------------>|   (ultimo chunk)
      |<---------------- ACK(N) --------------------|
      |                                            |
      |                                  (servidor recalcula SHA-256
      |                                   do arquivo montado)
      |<---------------- DONE(ok=1) ----------------|
      |                                            |
```

Se o cliente não receber o `ACK`/`NACK`/`DONE` esperado dentro do timeout
configurado (15s no `android-app`), a transferência é abortada com erro
sem derrubar a conexão de outros clientes no servidor.
