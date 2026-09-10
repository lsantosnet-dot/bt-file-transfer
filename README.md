# bt-file-transfer

Transferência de arquivos entre **Windows** e **Android** via **Bluetooth
Classic (RFCOMM/SPP)** — sem uso de Wi-Fi, internet ou cabo físico.

O Android atua como **cliente** (quem escolhe e envia o arquivo) e o
Windows atua como **servidor** (quem recebe e salva o arquivo). A
transferência é, por enquanto, unidirecional: apenas Android → Windows.

## Estrutura do monorepo

```
bt-file-transfer/
├── windows-server/     # App Rust para Windows (servidor RFCOMM)
├── android-app/        # App Flutter para Android (cliente RFCOMM)
├── docs/                # Documentação do protocolo compartilhado
└── README.md            # Este arquivo
```

- **`windows-server/`** — servidor Rust que usa a crate `windows`
  (bindings oficiais WinRT: `Devices.Bluetooth.Rfcomm`,
  `Networking.Sockets`, `Storage.Streams`) para publicar um serviço
  RFCOMM, receber o arquivo em chunks de 4KB e verificar sua integridade
  via SHA-256.
- **`android-app/`** — app Flutter que usa `flutter_bluetooth_serial_plus`
  (fork mantido do `flutter_bluetooth_serial`, mesma API) para conectar a
  um dispositivo pareado e enviar um arquivo escolhido pelo usuário, com
  barra de progresso.
- **`docs/protocol.md`** — especificação completa do protocolo binário
  de pacotes compartilhado pelos dois apps (formato do frame, os 6 tipos
  de pacote, payloads de `META`/`CHUNK`, diagrama de sequência e o UUID
  fixo do serviço RFCOMM).

## Passo obrigatório: parear os dispositivos

**Antes de usar qualquer um dos apps**, o Windows e o Android precisam
estar pareados via Bluetooth pelo sistema operacional (não é o app quem
faz o pareamento):

1. No Windows: **Configurações → Bluetooth e dispositivos → Adicionar
   dispositivo** → selecione o celular Android e confirme o PIN.
2. No Android: **Configurações → Conexões → Bluetooth** → selecione o PC
   Windows e confirme o mesmo PIN.
3. Verifique que ambos aparecem como "Pareado" nas respectivas telas de
   Bluetooth antes de abrir o `windows-server` ou o `android-app`.

Sem esse pareamento prévio pelo SO, o app Android não encontrará o
Windows na lista de dispositivos pareados (`getBondedDevices`), e a
conexão RFCOMM (`connect.toAddress`) falhará.

## Como compilar e rodar o `windows-server`

Requisitos: Windows 10/11 com adaptador Bluetooth, Rust (via
[rustup](https://rustup.rs)) e as ferramentas de build do Visual Studio
(MSVC). As APIs `Windows.Devices.Bluetooth.*` usadas aqui só existem em
tempo de execução no Windows — em outros sistemas o binário compila,
mas apenas imprime um aviso ao rodar (o módulo `protocol.rs`, que não
depende do Windows, continua testável em qualquer SO).

```bash
cd windows-server
cargo run
```

O servidor:

1. Publica um serviço RFCOMM com UUID fixo `818711c5-3946-4523-b54b-20ac27970afe`
   (veja `docs/protocol.md`).
2. Aguarda conexões, faz o handshake `HELLO`, recebe o pacote `META`
   (nome, tamanho, SHA-256) e o arquivo em chunks de 4KB, confirmando
   cada um com `ACK`.
3. Ao final, recalcula o SHA-256 do arquivo montado, confirma a
   integridade com um pacote `DONE` e salva o arquivo em
   `C:\BtFileTransfer\recebidos` (pasta criada automaticamente se não
   existir).

Para rodar os testes unitários do protocolo (framing de `HELLO`, `META`,
`CHUNK`, `ACK`, `NACK`, `DONE`, cálculo de CRC32, detecção de corrupção):

```bash
cd windows-server
cargo test
```

## Como rodar o `android-app`

Requisitos: [Flutter SDK](https://docs.flutter.dev/get-started/install) e
um dispositivo Android físico (Bluetooth Classic/SPP não funciona em
emuladores).

```bash
cd android-app
flutter pub get
flutter run
```

Ao abrir, o app solicita as permissões runtime `BLUETOOTH_CONNECT` e
`BLUETOOTH_SCAN` (obrigatórias a partir do Android 12), lista os
dispositivos já pareados em um dropdown, permite escolher um arquivo e
mostra uma barra de progresso e status textual durante o envio.

Análise estática e testes:

```bash
flutter analyze
flutter test
```

## Fluxo de uso completo

1. Parear Windows e Android pelo Bluetooth do sistema operacional (veja
   seção acima).
2. Rodar `cargo run` no `windows-server` (deixa o PC "escutando").
3. Abrir o `android-app`, selecionar o Windows na lista de dispositivos
   pareados e escolher o arquivo a enviar.
4. Tocar em "Enviar via Bluetooth" e acompanhar o progresso.
5. O arquivo aparece em `C:\BtFileTransfer\recebidos` no Windows, com a
   integridade já verificada via SHA-256.
