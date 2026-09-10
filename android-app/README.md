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

### JDK necessário para o build Android (Gradle)

Este projeto usa **Gradle 8.13 / AGP 8.13.0** — a última versão da
série 8.x, escolhida deliberadamente para ficar abaixo do Gradle 9.0
(que removeu de vez o método `jcenter()`, ainda usado pelo
`build.gradle` interno da dependência `flutter_bluetooth_serial`,
abandonada há anos). Essa combinação exige **JDK 17** (não roda em
JDK 20 ou anterior nem foi validada em JDK 21+).

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
