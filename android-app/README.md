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

## Testes e análise estática

```bash
flutter analyze
flutter test
```
