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

Este projeto usa Gradle 8.3 / AGP 8.1.0, versões exigidas pela
dependência `flutter_bluetooth_serial` (não recebe atualizações há
anos e não funciona em Gradle 9+/AGP 9+ — quebra ao resolver `jcenter()`,
removido do Gradle). Esse Gradle/AGP só rodam em **JDK até a versão
20**.

Se o seu `flutter doctor -v` mostrar uma "Java version" 21 ou mais
recente (comum em instalações novas do Android Studio, que já vêm com
JDK 25 embutido), o build falha com
`Unsupported class file major version ...`. Corrija apontando o
Flutter para um JDK 17 ou 21 dedicado, sem mudar o JDK do Android
Studio:

```bash
flutter config --jdk-dir="<caminho-do-jdk-17-ou-21>"
```

(ex.: instale o [Temurin 17](https://adoptium.net) e use o caminho de
instalação dele). Depois rode `flutter doctor -v` de novo para
confirmar a mudança.

## Testes e análise estática

```bash
flutter analyze
flutter test
```
