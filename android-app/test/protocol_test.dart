import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:android_app/protocol.dart';

void main() {
  test('crc32 bate com o vetor de teste padrao', () {
    expect(crc32('123456789'.codeUnits), 0xCBF43926);
  });

  test('HELLO faz round-trip', () {
    final packet = Packet.hello();
    final decoded = Packet.fromBytes(packet.toBytes());
    expect(decoded.packetType, PacketType.hello);
    expect(decoded.parseHello(), kProtocolVersion);
  });

  test('META faz round-trip preservando nome unicode', () {
    final hash = Uint8List.fromList(List.generate(32, (i) => i));
    final packet = Packet.meta(fileName: 'relatório_ç.pdf', fileSize: 123456, sha256: hash);
    final decoded = Packet.fromBytes(packet.toBytes());
    final meta = decoded.parseMeta();
    expect(meta.fileName, 'relatório_ç.pdf');
    expect(meta.fileSize, 123456);
    expect(meta.sha256, hash);
  });

  test('CHUNK faz round-trip com indice e dados', () {
    final data = Uint8List.fromList(List.filled(kChunkSize, 0xAA));
    final packet = Packet.chunk(42, data);
    final decoded = Packet.fromBytes(packet.toBytes());
    final (index, chunkData) = decoded.parseChunk();
    expect(index, 42);
    expect(chunkData, data);
  });

  test('ACK e NACK fazem round-trip', () {
    final ack = Packet.fromBytes(Packet.ack(7).toBytes());
    expect(ack.parseAck(), 7);

    final nack = Packet.fromBytes(Packet.nack(3, NackReason.crcMismatch).toBytes());
    final (index, reason) = nack.parseNack();
    expect(index, 3);
    expect(reason, NackReason.crcMismatch);
  });

  test('DONE carrega o status de sucesso/falha', () {
    expect(Packet.fromBytes(Packet.done(true).toBytes()).parseDone(), isTrue);
    expect(Packet.fromBytes(Packet.done(false).toBytes()).parseDone(), isFalse);
  });

  test('detecta payload corrompido via CRC32', () {
    final bytes = Packet.ack(1).toBytes();
    bytes[kHeaderLen] ^= 0xFF;
    expect(() => Packet.fromBytes(bytes), throwsA(isA<ProtocolException>()));
  });

  test('rejeita frame truncado', () {
    final bytes = Packet.meta(fileName: 'a.txt', fileSize: 10, sha256: Uint8List(32)).toBytes();
    final truncated = bytes.sublist(0, bytes.length - 5);
    expect(() => Packet.fromBytes(truncated), throwsA(isA<ProtocolException>()));
  });

  test('parser rejeita tipo de pacote inesperado', () {
    final packet = Packet.ack(1);
    expect(() => packet.parseMeta(), throwsA(isA<ProtocolException>()));
  });
}
