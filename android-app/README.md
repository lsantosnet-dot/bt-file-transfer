# android-app

Cliente Android (Flutter) do **bt-file-transfer**: envia arquivos para o
`windows-server` via Bluetooth Classic (RFCOMM/SPP), sem uso de Wi-Fi ou
cabo físico.

Veja o [README raiz](../README.md) para instruções completas de uso e
[`docs/protocol.md`](../docs/protocol.md) para a especificação do
protocolo.

## Estrutura

- `lib/protocol.dart` — framing de pacotes (tipo + tamanho + payload + CRC32),
  espelho do `windows-server/src/protocol.rs`.
- `lib/file_transfer_service.dart` — conexão RFCOMM, handshake, envio de
  metadados e chunks com confirmação (ACK/NACK), progresso e timeouts.
- `lib/main.dart` — tela única: dropdown de dispositivos pareados, seleção
  de arquivo, envio com barra de progresso.

## Rodando

```bash
flutter pub get
flutter run
```

Pré-requisito: o dispositivo Android precisa já estar **pareado** com o
Windows pelas configurações de Bluetooth do próprio Android antes de
abrir o app.

### Sobre a dependência de Bluetooth

O app usa **`flutter_bluetooth_serial_plus`**, um fork mantido do
`flutter_bluetooth_serial` com exatamente a mesma API Dart. O pacote
original parou de receber atualizações em 2021 e não compila com a
AGP 8: ele chama `jcenter()` (removido do Gradle 9), não declara
`namespace`, fixa `compileSdkVersion 30` (as libs AndroidX que ele
puxa exigem 34+) e ainda define `package` no `AndroidManifest.xml`
(a AGP 8 rejeita) — e nada disso dá para corrigir de fora, já que
esses arquivos vivem dentro do pacote baixado no pub cache.

### JDK necessário para o build Android (Gradle)

Este projeto usa **Gradle 8.14.2 / AGP 8.13.0**, combinação que exige
**JDK 17**.

Se o seu `flutter doctor -v` mostrar uma "Java version" diferente de
17 (comum em instalações novas do Android Studio, que já vêm com JDK
25 embutido), o build falha com `Unsupported class file major
version ...` ou erros de resolução de plugin. Corrija apontando o
Flutter para um JDK 17 dedicado, sem mudar o JDK do Android Studio:

```bash
flutter config --jdk-dir="<caminho-do-jdk-17>"
```

(ex.: instale o [Temurin 17](https://adoptium.net), ou aponte para um
JDK 17 que já exista na máquina — o Android Studio/IntelliJ costuma
manter um em `%USERPROFILE%\.jdks\`). Depois rode `flutter doctor -v`
de novo para confirmar a mudança.

## Testes e análise estática

```bash
flutter analyze
flutter test
```
