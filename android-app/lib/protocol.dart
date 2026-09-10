/// Framing do protocolo binário compartilhado com o servidor Rust
/// (`windows-server/src/protocol.rs`). Qualquer alteração de formato
/// precisa ser refletida nos dois lados. Veja `docs/protocol.md` para a
/// especificação completa.
///
/// Formato de um frame (inteiros multi-byte em little-endian):
///
/// ```text
/// +----------+----------------+-----------------+-----------+
/// | tipo (1) | tamanho_lp (4) | payload (N)     | crc32 (4) |
/// +----------+----------------+-----------------+-----------+
/// ```
///
/// O CRC32 cobre `tipo + tamanho_lp + payload`.
library protocol;

import 'dart:convert';
import 'dart:typed_data';

/// Tamanho máximo de payload aceito.
const int kMaxPayloadLen = 1024 * 1024; // 1 MiB

/// Tamanho de cada chunk de arquivo enviado, em bytes.
const int kChunkSize = 4096;

/// Tamanho do cabeçalho fixo (tipo + tamanho do payload).
const int kHeaderLen = 1 + 4;

/// Tamanho do trailer de CRC32.
const int kCrcLen = 4;

/// Versão atual do protocolo, enviada no payload do HELLO.
const int kProtocolVersion = 1;

enum PacketType {
  hello(0x01),
  meta(0x02),
  chunk(0x03),
  ack(0x04),
  nack(0x05),
  done(0x06);

  const PacketType(this.value);
  final int value;

  static PacketType? fromByte(int value) {
    for (final type in PacketType.values) {
      if (type.value == value) return type;
    }
    return null;
  }
}

enum NackReason {
  crcMismatch(0),
  ioError(1),
  outOfOrder(2),
  other(255);

  const NackReason(this.value);
  final int value;

  static NackReason fromByte(int value) {
    switch (value) {
      case 0:
        return NackReason.crcMismatch;
      case 1:
        return NackReason.ioError;
      case 2:
        return NackReason.outOfOrder;
      default:
        return NackReason.other;
    }
  }
}

/// Erro de protocolo, espelhando `ProtocolError` do lado Rust.
class ProtocolException implements Exception {
  ProtocolException(this.message);
  final String message;

  @override
  String toString() => 'ProtocolException: $message';
}

class MetaPayload {
  MetaPayload({required this.fileName, required this.fileSize, required this.sha256});
  final String fileName;
  final int fileSize;
  final Uint8List sha256;
}

// ---------------------------------------------------------------------
// CRC32 (IEEE 802.3 / zlib), implementado manualmente via tabela — sem
// depender de nenhum pacote externo de CRC, para casar 1:1 com a
// implementação também manual do lado Rust.
// ---------------------------------------------------------------------

final List<int> _crc32Table = _buildCrc32Table();

List<int> _buildCrc32Table() {
  final table = List<int>.filled(256, 0);
  for (var i = 0; i < 256; i++) {
    var c = i;
    for (var j = 0; j < 8; j++) {
      if (c & 1 != 0) {
        c = 0xEDB88320 ^ (c >> 1);
      } else {
        c = c >> 1;
      }
    }
    table[i] = c & 0xFFFFFFFF;
  }
  return table;
}

/// Calcula o CRC-32 (IEEE 802.3) padrão de um buffer.
int crc32(List<int> data) {
  var crc = 0xFFFFFFFF;
  for (final byte in data) {
    final index = (crc ^ byte) & 0xFF;
    crc = (_crc32Table[index] ^ (crc >> 8)) & 0xFFFFFFFF;
  }
  return crc ^ 0xFFFFFFFF;
}

// ---------------------------------------------------------------------
// Packet
// ---------------------------------------------------------------------

class Packet {
  Packet(this.packetType, this.payload);

  final PacketType packetType;
  final Uint8List payload;

  /// Serializa o pacote completo (cabeçalho + payload + CRC32) pronto
  /// para ser escrito na conexão RFCOMM.
  Uint8List toBytes() {
    final builder = BytesBuilder();
    builder.addByte(packetType.value);
    builder.add(_uint32le(payload.length));
    builder.add(payload);
    final head = builder.toBytes();
    final crc = crc32(head);
    builder.add(_uint32le(crc));
    return builder.toBytes();
  }

  /// Lê apenas o cabeçalho fixo para descobrir o tipo do pacote e o
  /// tamanho do payload que ainda precisa ser lido do stream.
  static (PacketType, int) decodeHeader(Uint8List header) {
    if (header.length < kHeaderLen) {
      throw ProtocolException('frame incompleto');
    }
    final type = PacketType.fromByte(header[0]);
    if (type == null) {
      throw ProtocolException(
          'tipo de pacote invalido: 0x${header[0].toRadixString(16).padLeft(2, '0')}');
    }
    final len = _readUint32le(header, 1);
    if (len > kMaxPayloadLen) {
      throw ProtocolException('payload muito grande: $len bytes');
    }
    return (type, len);
  }

  /// Decodifica um frame completo (cabeçalho + payload + CRC) já
  /// disponível em memória, validando o CRC32.
  static Packet fromBytes(Uint8List buf) {
    if (buf.length < kHeaderLen + kCrcLen) {
      throw ProtocolException('frame incompleto');
    }
    final (type, len) = decodeHeader(buf.sublist(0, kHeaderLen));
    final payloadEnd = kHeaderLen + len;
    final frameEnd = payloadEnd + kCrcLen;
    if (buf.length < frameEnd) {
      throw ProtocolException('frame incompleto');
    }

    final payload = buf.sublist(kHeaderLen, payloadEnd);
    final expectedCrc = _readUint32le(buf, payloadEnd);
    final actualCrc = crc32(buf.sublist(0, payloadEnd));
    if (expectedCrc != actualCrc) {
      throw ProtocolException(
          'crc32 invalido: esperado 0x${expectedCrc.toRadixString(16)}, '
          'recebido 0x${actualCrc.toRadixString(16)}');
    }

    return Packet(type, Uint8List.fromList(payload));
  }

  // -- Construtores para cada tipo de pacote -------------------------

  factory Packet.hello() => Packet(PacketType.hello, Uint8List.fromList([kProtocolVersion]));

  factory Packet.meta({
    required String fileName,
    required int fileSize,
    required Uint8List sha256,
  }) {
    final nameBytes = utf8.encode(fileName);
    final builder = BytesBuilder();
    builder.add(_uint16le(nameBytes.length));
    builder.add(nameBytes);
    builder.add(_uint64le(fileSize));
    builder.add(sha256);
    return Packet(PacketType.meta, builder.toBytes());
  }

  factory Packet.chunk(int index, Uint8List data) {
    final builder = BytesBuilder();
    builder.add(_uint32le(index));
    builder.add(data);
    return Packet(PacketType.chunk, builder.toBytes());
  }

  factory Packet.ack(int index) => Packet(PacketType.ack, Uint8List.fromList(_uint32le(index)));

  factory Packet.nack(int index, NackReason reason) {
    final builder = BytesBuilder();
    builder.add(_uint32le(index));
    builder.addByte(reason.value);
    return Packet(PacketType.nack, builder.toBytes());
  }

  factory Packet.done(bool success) =>
      Packet(PacketType.done, Uint8List.fromList([success ? 1 : 0]));

  // -- Parsers de payload por tipo ------------------------------------

  int parseHello() {
    _expectType(PacketType.hello);
    if (payload.isEmpty) throw ProtocolException('HELLO vazio');
    return payload[0];
  }

  MetaPayload parseMeta() {
    _expectType(PacketType.meta);
    if (payload.length < 2) {
      throw ProtocolException('META menor que o cabecalho de nome');
    }
    final nameLen = _readUint16le(payload, 0);
    const nameStart = 2;
    final nameEnd = nameStart + nameLen;
    final sizeEnd = nameEnd + 8;
    final hashEnd = sizeEnd + 32;
    if (payload.length < hashEnd) {
      throw ProtocolException('META truncado');
    }
    final fileName = utf8.decode(payload.sublist(nameStart, nameEnd));
    final fileSize = _readUint64le(payload, nameEnd);
    final sha256 = Uint8List.fromList(payload.sublist(sizeEnd, hashEnd));
    return MetaPayload(fileName: fileName, fileSize: fileSize, sha256: sha256);
  }

  (int, Uint8List) parseChunk() {
    _expectType(PacketType.chunk);
    if (payload.length < 4) {
      throw ProtocolException('CHUNK menor que o indice');
    }
    final index = _readUint32le(payload, 0);
    return (index, Uint8List.fromList(payload.sublist(4)));
  }

  int parseAck() {
    _expectType(PacketType.ack);
    if (payload.length < 4) {
      throw ProtocolException('ACK menor que o indice');
    }
    return _readUint32le(payload, 0);
  }

  (int, NackReason) parseNack() {
    _expectType(PacketType.nack);
    if (payload.length < 5) {
      throw ProtocolException('NACK menor que indice+motivo');
    }
    final index = _readUint32le(payload, 0);
    return (index, NackReason.fromByte(payload[4]));
  }

  bool parseDone() {
    _expectType(PacketType.done);
    if (payload.isEmpty) throw ProtocolException('DONE vazio');
    return payload[0] != 0;
  }

  void _expectType(PacketType expected) {
    if (packetType != expected) {
      throw ProtocolException('esperava pacote $expected, recebeu $packetType');
    }
  }
}

// -- Helpers de codificação binária ------------------------------------

List<int> _uint16le(int v) => [v & 0xFF, (v >> 8) & 0xFF];

List<int> _uint32le(int v) => [
      v & 0xFF,
      (v >> 8) & 0xFF,
      (v >> 16) & 0xFF,
      (v >> 24) & 0xFF,
    ];

List<int> _uint64le(int v) {
  final bytes = List<int>.filled(8, 0);
  var value = v;
  for (var i = 0; i < 8; i++) {
    bytes[i] = value & 0xFF;
    value >>= 8;
  }
  return bytes;
}

int _readUint16le(Uint8List buf, int offset) => buf[offset] | (buf[offset + 1] << 8);

int _readUint32le(Uint8List buf, int offset) =>
    buf[offset] | (buf[offset + 1] << 8) | (buf[offset + 2] << 16) | (buf[offset + 3] << 24);

int _readUint64le(Uint8List buf, int offset) {
  var value = 0;
  for (var i = 7; i >= 0; i--) {
    value = (value << 8) | buf[offset + i];
  }
  return value;
}
