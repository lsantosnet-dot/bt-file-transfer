/// Serviço responsável por conduzir uma transferência de arquivo do
/// Android para o servidor Windows via Bluetooth Classic (RFCOMM/SPP),
/// usando o protocolo de pacotes definido em `protocol.dart`.
library file_transfer_service;

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:crypto/crypto.dart';
import 'package:flutter_bluetooth_serial/flutter_bluetooth_serial.dart';

import 'protocol.dart';

/// Erro específico de uma transferência de arquivo (distinto de erros
/// de framing do protocolo, que usam [ProtocolException]).
class FileTransferException implements Exception {
  FileTransferException(this.message);
  final String message;

  @override
  String toString() => 'FileTransferException: $message';
}

/// Quantidade máxima de vezes que um mesmo chunk é reenviado antes de
/// desistir da transferência.
const int _maxChunkRetries = 3;

/// Tempo máximo de espera por uma resposta (ACK/NACK/HELLO/DONE) antes
/// de considerar a conexão travada.
const Duration _ackTimeout = Duration(seconds: 15);

/// Lê o [Stream] de bytes bruto de uma [BluetoothConnection] e permite
/// consumi-lo como pacotes completos do protocolo, escondendo o fato de
/// que o Bluetooth Classic entrega os bytes em pedaços arbitrários (o
/// SO pode fragmentar ou juntar escritas de formas que não têm relação
/// com os limites dos pacotes).
class _FrameReader {
  _FrameReader(Stream<Uint8List> input) {
    _subscription = input.listen(_onData, onError: _onError, onDone: _onDone);
  }

  final List<int> _buffer = [];
  final List<_PendingRead> _pending = [];
  StreamSubscription<Uint8List>? _subscription;
  Object? _closedWith;

  void _onData(Uint8List chunk) {
    _buffer.addAll(chunk);
    _fulfillPending();
  }

  void _onError(Object error) {
    _closedWith = error;
    for (final pending in _pending) {
      pending.completer.completeError(error);
    }
    _pending.clear();
  }

  void _onDone() {
    _closedWith ??= FileTransferException('Conexao Bluetooth encerrada pelo dispositivo remoto.');
    for (final pending in _pending) {
      pending.completer.completeError(_closedWith!);
    }
    _pending.clear();
  }

  void _fulfillPending() {
    while (_pending.isNotEmpty && _buffer.length >= _pending.first.length) {
      final pending = _pending.removeAt(0);
      final data = Uint8List.fromList(_buffer.sublist(0, pending.length));
      _buffer.removeRange(0, pending.length);
      pending.completer.complete(data);
    }
  }

  Future<Uint8List> _readExact(int length) {
    if (_closedWith != null) {
      return Future.error(_closedWith!);
    }
    if (_buffer.length >= length) {
      final data = Uint8List.fromList(_buffer.sublist(0, length));
      _buffer.removeRange(0, length);
      return Future.value(data);
    }
    final completer = Completer<Uint8List>();
    _pending.add(_PendingRead(length, completer));
    return completer.future;
  }

  /// Lê um frame completo do protocolo: primeiro o cabeçalho fixo (para
  /// saber o tamanho do payload), depois o payload + CRC32.
  Future<Packet> readPacket({Duration timeout = _ackTimeout}) {
    return _readFrame().timeout(
      timeout,
      onTimeout: () => throw TimeoutException('Tempo esgotado aguardando resposta do dispositivo remoto.'),
    );
  }

  Future<Packet> _readFrame() async {
    final header = await _readExact(kHeaderLen);
    final (_, payloadLen) = Packet.decodeHeader(header);
    final rest = await _readExact(payloadLen + kCrcLen);
    final frame = Uint8List.fromList([...header, ...rest]);
    return Packet.fromBytes(frame);
  }

  Future<void> dispose() async {
    await _subscription?.cancel();
  }
}

class _PendingRead {
  _PendingRead(this.length, this.completer);
  final int length;
  final Completer<Uint8List> completer;
}

/// Coordena o envio de um arquivo para o servidor Windows: conexão RFCOMM,
/// handshake, metadados, chunks com confirmação e verificação final de
/// integridade.
class FileTransferService {
  BluetoothConnection? _connection;
  _FrameReader? _reader;

  /// Conecta a um dispositivo Bluetooth já pareado (via configurações do
  /// Android). O pareamento em si precisa ter sido feito previamente
  /// pelo usuário — este app não realiza pareamento.
  Future<void> connect(BluetoothDevice device) async {
    final connection = await BluetoothConnection.toAddress(device.address);
    final input = connection.input;
    if (input == null) {
      await connection.close();
      throw FileTransferException('Nao foi possivel abrir o stream de entrada da conexao Bluetooth.');
    }
    _connection = connection;
    _reader = _FrameReader(input);
  }

  Future<void> disconnect() async {
    await _reader?.dispose();
    _reader = null;
    final connection = _connection;
    _connection = null;
    if (connection != null && connection.isConnected) {
      await connection.finish();
    }
  }

  bool get isConnected => _connection?.isConnected ?? false;

  /// Envia [file] pela conexão já estabelecida, chamando [onProgress]
  /// com um valor entre 0.0 e 1.0 conforme os chunks são confirmados, e
  /// [onStatus] com mensagens textuais sobre a etapa atual.
  Future<void> sendFile(
    File file, {
    required void Function(double progress) onProgress,
    void Function(String status)? onStatus,
  }) async {
    final connection = _connection;
    final reader = _reader;
    if (connection == null || reader == null) {
      throw FileTransferException('Chame connect() antes de sendFile().');
    }

    onStatus?.call('Lendo arquivo e calculando SHA-256...');
    final bytes = await file.readAsBytes();
    final hash = Uint8List.fromList(sha256.convert(bytes).bytes);
    final fileName = _basename(file.path);

    onStatus?.call('Enviando handshake (HELLO)...');
    await _writePacket(connection, Packet.hello());
    final helloResponse = await reader.readPacket();
    if (helloResponse.packetType != PacketType.hello) {
      throw FileTransferException('Resposta inesperada ao HELLO: ${helloResponse.packetType}');
    }
    helloResponse.parseHello();

    onStatus?.call('Enviando metadados do arquivo...');
    await _writePacket(
      connection,
      Packet.meta(fileName: fileName, fileSize: bytes.length, sha256: hash),
    );
    final metaResponse = await reader.readPacket();
    if (metaResponse.packetType != PacketType.ack) {
      throw FileTransferException('Servidor rejeitou os metadados (${metaResponse.packetType}).');
    }

    final totalChunks = bytes.isEmpty ? 0 : (bytes.length / kChunkSize).ceil();
    onStatus?.call('Enviando arquivo em $totalChunks chunk(s)...');
    for (var index = 0; index < totalChunks; index++) {
      final start = index * kChunkSize;
      final end = math.min(start + kChunkSize, bytes.length);
      final chunkData = Uint8List.sublistView(bytes, start, end);
      await _sendChunkWithRetry(connection, reader, index, chunkData);
      onProgress(totalChunks == 0 ? 1.0 : (index + 1) / totalChunks);
    }

    onStatus?.call('Aguardando confirmacao de integridade do servidor...');
    final donePacket = await reader.readPacket();
    final success = donePacket.parseDone();
    if (!success) {
      throw FileTransferException(
          'O servidor recebeu o arquivo, mas a verificacao de integridade (SHA-256) falhou.');
    }
    onStatus?.call('Transferencia concluida com sucesso.');
  }

  Future<void> _sendChunkWithRetry(
    BluetoothConnection connection,
    _FrameReader reader,
    int index,
    Uint8List data,
  ) async {
    for (var attempt = 1; attempt <= _maxChunkRetries; attempt++) {
      await _writePacket(connection, Packet.chunk(index, data));

      final Packet response;
      try {
        response = await reader.readPacket();
      } on TimeoutException {
        if (attempt == _maxChunkRetries) rethrow;
        continue;
      }

      if (response.packetType == PacketType.ack) {
        final ackedIndex = response.parseAck();
        if (ackedIndex == index) return;
        // ACK de um índice diferente do esperado: trata como resposta
        // inválida e tenta reenviar o chunk atual.
      } else if (response.packetType == PacketType.nack) {
        // NACK: servidor pediu reenvio (chunk corrompido ou fora de
        // ordem); tenta novamente até o limite de tentativas.
      } else {
        throw FileTransferException('Resposta inesperada ao CHUNK $index: ${response.packetType}');
      }
    }
    throw FileTransferException('Falha ao enviar o chunk $index apos $_maxChunkRetries tentativas.');
  }

  Future<void> _writePacket(BluetoothConnection connection, Packet packet) async {
    connection.output.add(packet.toBytes());
    await connection.output.allSent;
  }
}

String _basename(String path) {
  final normalized = path.replaceAll('\\', '/');
  final lastSlash = normalized.lastIndexOf('/');
  return lastSlash == -1 ? normalized : normalized.substring(lastSlash + 1);
}
