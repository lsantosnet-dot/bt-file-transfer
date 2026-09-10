//! Framing do protocolo binário compartilhado com o app Android.
//!
//! Formato de um frame (todos os inteiros multi-byte em little-endian):
//!
//! ```text
//! +----------+----------------+-----------------+-----------+
//! | tipo (1) | tamanho_lp (4) | payload (N)     | crc32 (4) |
//! +----------+----------------+-----------------+-----------+
//! ```
//!
//! O CRC32 é calculado sobre `tipo + tamanho_lp + payload` (ou seja, tudo
//! menos o próprio campo de CRC). O mesmo layout é implementado de forma
//! independente em `lib/protocol.dart` no app Flutter — qualquer mudança
//! aqui precisa ser replicada lá. Veja `docs/protocol.md` para a
//! especificação completa.

use std::fmt;

/// Tamanho máximo de payload aceito (proteção contra alocação
/// desenfreada caso um frame corrompido informe um tamanho absurdo).
pub const MAX_PAYLOAD_LEN: usize = 1024 * 1024; // 1 MiB

/// Tamanho de cada chunk de arquivo transmitido, em bytes.
pub const CHUNK_SIZE: usize = 4096;

/// Tamanho do cabeçalho fixo (tipo + tamanho do payload).
pub const HEADER_LEN: usize = 1 + 4;

/// Tamanho do trailer de CRC32.
pub const CRC_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketType {
    /// Handshake inicial, primeiro pacote trocado na conexão.
    Hello = 0x01,
    /// Metadados do arquivo (nome, tamanho, hash SHA-256).
    Meta = 0x02,
    /// Um pedaço (chunk) do conteúdo do arquivo.
    Chunk = 0x03,
    /// Confirmação positiva de recebimento de um pacote.
    Ack = 0x04,
    /// Confirmação negativa (pacote rejeitado / corrompido).
    Nack = 0x05,
    /// Sinaliza o fim da transferência e o resultado da verificação de integridade.
    Done = 0x06,
}

impl PacketType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(PacketType::Hello),
            0x02 => Some(PacketType::Meta),
            0x03 => Some(PacketType::Chunk),
            0x04 => Some(PacketType::Ack),
            0x05 => Some(PacketType::Nack),
            0x06 => Some(PacketType::Done),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// O buffer fornecido não contém dados suficientes para um frame completo.
    Truncated,
    /// Byte de tipo de pacote desconhecido.
    InvalidType(u8),
    /// O CRC32 calculado não bate com o CRC32 recebido no frame.
    CrcMismatch { expected: u32, actual: u32 },
    /// O tamanho de payload declarado excede `MAX_PAYLOAD_LEN`.
    PayloadTooLarge(usize),
    /// Payload malformado para o tipo de pacote esperado (ex.: META curto demais).
    MalformedPayload(&'static str),
    /// Tipo de pacote inesperado no contexto atual (ex.: esperava ACK e veio CHUNK).
    UnexpectedType { expected: PacketType, actual: PacketType },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Truncated => write!(f, "frame incompleto"),
            ProtocolError::InvalidType(b) => write!(f, "tipo de pacote invalido: 0x{b:02x}"),
            ProtocolError::CrcMismatch { expected, actual } => {
                write!(f, "crc32 invalido: esperado 0x{expected:08x}, recebido 0x{actual:08x}")
            }
            ProtocolError::PayloadTooLarge(n) => write!(f, "payload muito grande: {n} bytes"),
            ProtocolError::MalformedPayload(msg) => write!(f, "payload malformado: {msg}"),
            ProtocolError::UnexpectedType { expected, actual } => {
                write!(f, "esperava pacote {expected:?}, recebeu {actual:?}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Um pacote decodificado: tipo + payload já sem o framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub packet_type: PacketType,
    pub payload: Vec<u8>,
}

/// Motivo de um NACK, transmitido como o segundo byte do payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NackReason {
    CrcMismatch = 0,
    IoError = 1,
    OutOfOrder = 2,
    Other = 255,
}

impl NackReason {
    pub fn from_u8(v: u8) -> NackReason {
        match v {
            0 => NackReason::CrcMismatch,
            1 => NackReason::IoError,
            2 => NackReason::OutOfOrder,
            _ => NackReason::Other,
        }
    }
}

/// Metadados de arquivo carregados por um pacote META.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaPayload {
    pub file_name: String,
    pub file_size: u64,
    pub sha256: [u8; 32],
}

// ---------------------------------------------------------------------
// CRC32 (IEEE 802.3 / zlib), implementado via tabela gerada em tempo de
// compilação. Reimplementado aqui (em vez de usar uma crate como
// `crc32fast`) para casar 1:1 com a implementação manual exigida do lado
// Dart e manter o módulo sem dependências externas.
// ---------------------------------------------------------------------

const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const CRC32_TABLE: [u32; 256] = build_crc32_table();

/// Calcula o CRC-32 (IEEE 802.3) padrão de um buffer.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[index] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------
// Codificação / decodificação de frames
// ---------------------------------------------------------------------

impl Packet {
    pub fn new(packet_type: PacketType, payload: Vec<u8>) -> Self {
        Self { packet_type, payload }
    }

    /// Serializa o pacote completo (cabeçalho + payload + CRC32) pronto
    /// para ser escrito no socket.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(HEADER_LEN + self.payload.len() + CRC_LEN);
        buf.push(self.packet_type.as_u8());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.payload);
        let crc = crc32(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    /// Lê apenas o cabeçalho fixo (`HEADER_LEN` bytes) para descobrir o
    /// tipo do pacote e o tamanho do payload que ainda precisa ser lido
    /// do stream. Útil para leitura incremental de um socket, onde não
    /// se sabe de antemão quantos bytes o frame completo ocupa.
    pub fn decode_header(header: &[u8]) -> Result<(PacketType, u32), ProtocolError> {
        if header.len() < HEADER_LEN {
            return Err(ProtocolError::Truncated);
        }
        let packet_type =
            PacketType::from_u8(header[0]).ok_or(ProtocolError::InvalidType(header[0]))?;
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
        if len as usize > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::PayloadTooLarge(len as usize));
        }
        Ok((packet_type, len))
    }

    /// Decodifica um frame completo (cabeçalho + payload + CRC) já
    /// disponível em memória, validando o CRC32.
    pub fn from_bytes(buf: &[u8]) -> Result<Packet, ProtocolError> {
        if buf.len() < HEADER_LEN + CRC_LEN {
            return Err(ProtocolError::Truncated);
        }
        let (packet_type, len) = Self::decode_header(&buf[..HEADER_LEN])?;
        let len = len as usize;
        let payload_end = HEADER_LEN + len;
        let frame_end = payload_end + CRC_LEN;
        if buf.len() < frame_end {
            return Err(ProtocolError::Truncated);
        }

        let payload = &buf[HEADER_LEN..payload_end];
        let expected_crc =
            u32::from_le_bytes(buf[payload_end..frame_end].try_into().unwrap());
        let actual_crc = crc32(&buf[..payload_end]);
        if expected_crc != actual_crc {
            return Err(ProtocolError::CrcMismatch { expected: expected_crc, actual: actual_crc });
        }

        Ok(Packet { packet_type, payload: payload.to_vec() })
    }

    // -- Construtores para cada tipo de pacote -------------------------

    pub fn hello() -> Packet {
        // Payload de 1 byte com a versão do protocolo, para permitir
        // evolução futura sem quebrar clientes antigos.
        Packet::new(PacketType::Hello, vec![PROTOCOL_VERSION])
    }

    pub fn meta(file_name: &str, file_size: u64, sha256: [u8; 32]) -> Packet {
        let name_bytes = file_name.as_bytes();
        let mut payload = Vec::with_capacity(2 + name_bytes.len() + 8 + 32);
        payload.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(name_bytes);
        payload.extend_from_slice(&file_size.to_le_bytes());
        payload.extend_from_slice(&sha256);
        Packet::new(PacketType::Meta, payload)
    }

    pub fn chunk(index: u32, data: &[u8]) -> Packet {
        let mut payload = Vec::with_capacity(4 + data.len());
        payload.extend_from_slice(&index.to_le_bytes());
        payload.extend_from_slice(data);
        Packet::new(PacketType::Chunk, payload)
    }

    pub fn ack(index: u32) -> Packet {
        Packet::new(PacketType::Ack, index.to_le_bytes().to_vec())
    }

    pub fn nack(index: u32, reason: NackReason) -> Packet {
        let mut payload = index.to_le_bytes().to_vec();
        payload.push(reason as u8);
        Packet::new(PacketType::Nack, payload)
    }

    pub fn done(success: bool) -> Packet {
        Packet::new(PacketType::Done, vec![success as u8])
    }

    // -- Parsers de payload por tipo ------------------------------------

    pub fn parse_hello(&self) -> Result<u8, ProtocolError> {
        self.expect_type(PacketType::Hello)?;
        self.payload
            .first()
            .copied()
            .ok_or(ProtocolError::MalformedPayload("HELLO vazio"))
    }

    pub fn parse_meta(&self) -> Result<MetaPayload, ProtocolError> {
        self.expect_type(PacketType::Meta)?;
        let p = &self.payload;
        if p.len() < 2 {
            return Err(ProtocolError::MalformedPayload("META menor que o cabecalho de nome"));
        }
        let name_len = u16::from_le_bytes([p[0], p[1]]) as usize;
        let name_start = 2;
        let name_end = name_start + name_len;
        let size_end = name_end + 8;
        let hash_end = size_end + 32;
        if p.len() < hash_end {
            return Err(ProtocolError::MalformedPayload("META truncado"));
        }

        let file_name = String::from_utf8(p[name_start..name_end].to_vec())
            .map_err(|_| ProtocolError::MalformedPayload("nome de arquivo nao e UTF-8 valido"))?;
        let file_size = u64::from_le_bytes(p[name_end..size_end].try_into().unwrap());
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&p[size_end..hash_end]);

        Ok(MetaPayload { file_name, file_size, sha256 })
    }

    pub fn parse_chunk(&self) -> Result<(u32, &[u8]), ProtocolError> {
        self.expect_type(PacketType::Chunk)?;
        if self.payload.len() < 4 {
            return Err(ProtocolError::MalformedPayload("CHUNK menor que o indice"));
        }
        let index = u32::from_le_bytes(self.payload[0..4].try_into().unwrap());
        Ok((index, &self.payload[4..]))
    }

    pub fn parse_ack(&self) -> Result<u32, ProtocolError> {
        self.expect_type(PacketType::Ack)?;
        if self.payload.len() < 4 {
            return Err(ProtocolError::MalformedPayload("ACK menor que o indice"));
        }
        Ok(u32::from_le_bytes(self.payload[0..4].try_into().unwrap()))
    }

    pub fn parse_nack(&self) -> Result<(u32, NackReason), ProtocolError> {
        self.expect_type(PacketType::Nack)?;
        if self.payload.len() < 5 {
            return Err(ProtocolError::MalformedPayload("NACK menor que indice+motivo"));
        }
        let index = u32::from_le_bytes(self.payload[0..4].try_into().unwrap());
        Ok((index, NackReason::from_u8(self.payload[4])))
    }

    pub fn parse_done(&self) -> Result<bool, ProtocolError> {
        self.expect_type(PacketType::Done)?;
        self.payload
            .first()
            .map(|&b| b != 0)
            .ok_or(ProtocolError::MalformedPayload("DONE vazio"))
    }

    fn expect_type(&self, expected: PacketType) -> Result<(), ProtocolError> {
        if self.packet_type != expected {
            return Err(ProtocolError::UnexpectedType { expected, actual: self.packet_type });
        }
        Ok(())
    }
}

/// Versão atual do protocolo, enviada no payload do HELLO.
pub const PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // Vetor de teste padrão: CRC32("123456789") = 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn hello_roundtrip() {
        let packet = Packet::hello();
        let bytes = packet.to_bytes();
        let decoded = Packet::from_bytes(&bytes).expect("deve decodificar");
        assert_eq!(decoded.packet_type, PacketType::Hello);
        assert_eq!(decoded.parse_hello().unwrap(), PROTOCOL_VERSION);
    }

    #[test]
    fn meta_roundtrip() {
        let hash = [7u8; 32];
        let packet = Packet::meta("arquivo de teste.txt", 123_456, hash);
        let bytes = packet.to_bytes();
        let decoded = Packet::from_bytes(&bytes).expect("deve decodificar");
        let meta = decoded.parse_meta().expect("deve parsear META");
        assert_eq!(meta.file_name, "arquivo de teste.txt");
        assert_eq!(meta.file_size, 123_456);
        assert_eq!(meta.sha256, hash);
    }

    #[test]
    fn meta_roundtrip_with_unicode_name() {
        let hash = [1u8; 32];
        let packet = Packet::meta("relatório_ç.pdf", 42, hash);
        let bytes = packet.to_bytes();
        let decoded = Packet::from_bytes(&bytes).unwrap();
        let meta = decoded.parse_meta().unwrap();
        assert_eq!(meta.file_name, "relatório_ç.pdf");
    }

    #[test]
    fn chunk_roundtrip() {
        let data = vec![0xAAu8; CHUNK_SIZE];
        let packet = Packet::chunk(42, &data);
        let bytes = packet.to_bytes();
        let decoded = Packet::from_bytes(&bytes).unwrap();
        let (index, chunk_data) = decoded.parse_chunk().unwrap();
        assert_eq!(index, 42);
        assert_eq!(chunk_data, data.as_slice());
    }

    #[test]
    fn chunk_roundtrip_partial_final_chunk() {
        let data = vec![0x01u8; 17];
        let packet = Packet::chunk(9, &data);
        let decoded = Packet::from_bytes(&packet.to_bytes()).unwrap();
        let (index, chunk_data) = decoded.parse_chunk().unwrap();
        assert_eq!(index, 9);
        assert_eq!(chunk_data, data.as_slice());
    }

    #[test]
    fn ack_roundtrip() {
        let packet = Packet::ack(7);
        let decoded = Packet::from_bytes(&packet.to_bytes()).unwrap();
        assert_eq!(decoded.parse_ack().unwrap(), 7);
    }

    #[test]
    fn nack_roundtrip() {
        let packet = Packet::nack(3, NackReason::CrcMismatch);
        let decoded = Packet::from_bytes(&packet.to_bytes()).unwrap();
        let (index, reason) = decoded.parse_nack().unwrap();
        assert_eq!(index, 3);
        assert_eq!(reason, NackReason::CrcMismatch);
    }

    #[test]
    fn done_roundtrip_success_and_failure() {
        let ok = Packet::done(true);
        assert!(Packet::from_bytes(&ok.to_bytes()).unwrap().parse_done().unwrap());

        let fail = Packet::done(false);
        assert!(!Packet::from_bytes(&fail.to_bytes()).unwrap().parse_done().unwrap());
    }

    #[test]
    fn detects_corrupted_payload_via_crc() {
        let packet = Packet::ack(1);
        let mut bytes = packet.to_bytes();
        // Corrompe um byte do payload sem tocar no CRC.
        let payload_index = HEADER_LEN;
        bytes[payload_index] ^= 0xFF;
        let err = Packet::from_bytes(&bytes).unwrap_err();
        assert!(matches!(err, ProtocolError::CrcMismatch { .. }));
    }

    #[test]
    fn rejects_unknown_packet_type() {
        let mut bytes = Packet::hello().to_bytes();
        bytes[0] = 0x99;
        // Recalcula o CRC para isolar o teste do erro de tipo.
        let crc = crc32(&bytes[..bytes.len() - CRC_LEN]);
        let crc_start = bytes.len() - CRC_LEN;
        bytes[crc_start..].copy_from_slice(&crc.to_le_bytes());
        let err = Packet::from_bytes(&bytes).unwrap_err();
        assert_eq!(err, ProtocolError::InvalidType(0x99));
    }

    #[test]
    fn rejects_truncated_frame() {
        let bytes = Packet::meta("a.txt", 10, [0u8; 32]).to_bytes();
        let truncated = &bytes[..bytes.len() - 5];
        assert_eq!(Packet::from_bytes(truncated).unwrap_err(), ProtocolError::Truncated);
    }

    #[test]
    fn decode_header_reports_payload_length() {
        let packet = Packet::chunk(1, &[0u8; 100]);
        let bytes = packet.to_bytes();
        let (packet_type, len) = Packet::decode_header(&bytes[..HEADER_LEN]).unwrap();
        assert_eq!(packet_type, PacketType::Chunk);
        assert_eq!(len as usize, 4 + 100);
    }

    #[test]
    fn expect_type_mismatch_is_reported() {
        let packet = Packet::ack(1);
        let err = packet.parse_meta().unwrap_err();
        assert_eq!(
            err,
            ProtocolError::UnexpectedType { expected: PacketType::Meta, actual: PacketType::Ack }
        );
    }
}
