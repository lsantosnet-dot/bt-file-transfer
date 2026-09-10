//! Servidor RFCOMM (Bluetooth Classic / SPP) baseado nas APIs WinRT
//! expostas pela crate `windows`.
//!
//! Este módulo só é compilado em `target_os = "windows"` (veja o
//! `Cargo.toml`, onde a dependência `windows` é declarada apenas para
//! `cfg(windows)`), pois as APIs `Windows.Devices.Bluetooth.*` só
//! existem no runtime do Windows 10/11.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use sha2::{Digest, Sha256};
use windows::core::{HSTRING, GUID};
use windows::Devices::Bluetooth::Rfcomm::{RfcommServiceId, RfcommServiceProvider};
use windows::Foundation::TypedEventHandler;
use windows::Networking::Sockets::{
    SocketProtectionLevel, StreamSocket, StreamSocketListener,
    StreamSocketListenerConnectionReceivedEventArgs,
};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

use crate::protocol::{self, NackReason, Packet, PacketType, CRC_LEN, HEADER_LEN};

/// UUID do serviço RFCOMM: o UUID padrão do Serial Port Profile (SPP),
/// `00001101-0000-1000-8000-00805F9B34FB`.
///
/// Ele NÃO pode ser trocado por um UUID próprio: o plugin Android
/// (`flutter_bluetooth_serial_plus`) chama
/// `createRfcommSocketToServiceRecord` com esse UUID fixo no código Java
/// e não expõe nenhuma forma de passar outro pelo Dart. Publicar aqui um
/// UUID diferente faz o SDP do celular não encontrar serviço nenhum, e a
/// conexão fica pendurada sem nunca completar.
pub const SERVICE_UUID: &str = "00001101-0000-1000-8000-00805F9B34FB";

/// Nome amigável anunciado no SDP record do serviço.
const SERVICE_DISPLAY_NAME: &str = "BT File Transfer";

/// Id do atributo SDP "ServiceName" e seu tipo (TextString), conforme a
/// especificação Bluetooth.
const SDP_SERVICE_NAME_ATTRIBUTE_ID: u32 = 0x0100;
const SDP_SERVICE_NAME_TYPE: u8 = 0x25;

/// Pasta padrão onde os arquivos recebidos são salvos.
fn default_download_dir() -> PathBuf {
    PathBuf::from(r"C:\BtFileTransfer\recebidos")
}

/// Inicia o provider RFCOMM, publica o serviço via SDP e escuta
/// conexões indefinidamente. Cada conexão é tratada de forma síncrona
/// (um cliente por vez), o que é suficiente para o caso de uso de
/// transferência ponto a ponto Android -> Windows.
pub fn run() -> windows::core::Result<()> {
    let download_dir = default_download_dir();
    std::fs::create_dir_all(&download_dir).expect("nao foi possivel criar a pasta de destino");

    let service_guid = GUID::try_from(SERVICE_UUID).expect("UUID de servico invalido");
    let service_id = RfcommServiceId::FromUuid(service_guid)?;

    // `CreateAsync` registra o serviço junto ao stack Bluetooth do SO;
    // `.get()` bloqueia a thread atual até a IAsyncOperation completar,
    // evitando a necessidade de um executor assíncrono só para isso.
    let provider = RfcommServiceProvider::CreateAsync(&service_id)?.get()?;

    let listener = StreamSocketListener::new()?;

    // Canal usado para trazer cada conexão aceita (que chega em uma
    // thread pool gerenciada pelo WinRT) de volta para processamento
    // sequencial, uma por vez.
    let (tx, rx) = mpsc::channel::<StreamSocket>();

    let handler = TypedEventHandler::new(
        move |_listener, args: &Option<StreamSocketListenerConnectionReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                if let Ok(socket) = args.Socket() {
                    let _ = tx.send(socket);
                }
            }
            Ok(())
        },
    );
    listener.ConnectionReceived(&handler)?;

    // Começa a escutar na porta RFCOMM alocada dinamicamente pelo SO.
    //
    // O nível de proteção precisa ser explícito: o Android abre o socket
    // com `createRfcommSocketToServiceRecord`, que exige um canal
    // *criptografado*. Se o listener ficar no padrão (sem criptografia),
    // o RFCOMM rejeita a conexão e o celular recebe um
    // "read failed, socket might closed or timeout" dentro de connect().
    //
    // Em WinRT esse overload tem nome próprio (não é o mesmo
    // `BindServiceNameAsync` com um argumento a mais).
    listener
        .BindServiceNameWithProtectionLevelAsync(
            &service_id.AsString()?,
            SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
        )?
        .get()?;

    // Preenche o registro SDP com o nome do serviço antes de anunciar.
    // Sem nenhum atributo o registro fica "pelado", e parte dos stacks
    // Bluetooth ignora ou trata mal um SPP sem ServiceName. É o mesmo
    // atributo que o exemplo RfcommChatServer da Microsoft escreve.
    let sdp_writer = DataWriter::new()?;
    sdp_writer.WriteByte(SDP_SERVICE_NAME_TYPE)?;
    sdp_writer.WriteByte(SERVICE_DISPLAY_NAME.len() as u8)?;
    sdp_writer.WriteString(&HSTRING::from(SERVICE_DISPLAY_NAME))?;
    provider
        .SdpRawAttributes()?
        .Insert(SDP_SERVICE_NAME_ATTRIBUTE_ID, &sdp_writer.DetachBuffer()?)?;

    provider.StartAdvertising(&listener)?;

    println!("bt-file-transfer: servidor RFCOMM ativo.");
    println!("  UUID do servico: {SERVICE_UUID} (Serial Port Profile)");
    println!("  Nome anunciado : {SERVICE_DISPLAY_NAME}");
    println!("  Pasta destino  : {}", download_dir.display());
    println!("Aguardando conexoes do app Android (pareie o dispositivo antes de enviar)...");

    // Loop principal: cada conexão aceita é tratada até o fim (ou até
    // falhar) antes de aceitarmos a próxima, uma desconexão inesperada
    // de um cliente nunca derruba o processo do servidor.
    for socket in rx {
        match handle_connection(&socket, &download_dir) {
            Ok(name) => println!("Transferencia concluida com sucesso: {name}"),
            Err(e) => eprintln!("Conexao encerrada com erro (servidor continua ativo): {e}"),
        }
    }

    Ok(())
}

#[derive(Debug)]
enum ServerError {
    WinRt(windows::core::Error),
    Protocol(protocol::ProtocolError),
    Io(std::io::Error),
    IntegrityCheckFailed,
    Disconnected,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::WinRt(e) => write!(f, "erro WinRT: {e}"),
            ServerError::Protocol(e) => write!(f, "erro de protocolo: {e}"),
            ServerError::Io(e) => write!(f, "erro de E/S: {e}"),
            ServerError::IntegrityCheckFailed => write!(f, "hash SHA-256 nao confere apos recebimento"),
            ServerError::Disconnected => write!(f, "cliente desconectou inesperadamente"),
        }
    }
}

impl From<windows::core::Error> for ServerError {
    fn from(e: windows::core::Error) -> Self {
        ServerError::WinRt(e)
    }
}
impl From<protocol::ProtocolError> for ServerError {
    fn from(e: protocol::ProtocolError) -> Self {
        ServerError::Protocol(e)
    }
}
impl From<std::io::Error> for ServerError {
    fn from(e: std::io::Error) -> Self {
        ServerError::Io(e)
    }
}

/// Processa uma conexão do início (HELLO) ao fim (DONE), persistindo o
/// arquivo recebido em `download_dir`.
fn handle_connection(socket: &StreamSocket, download_dir: &Path) -> Result<String, ServerError> {
    let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
    // `Partial` faz `LoadAsync` retornar assim que houver pelo menos 1
    // byte disponível, em vez de bloquear até encher o buffer pedido;
    // por isso sempre conferimos quantos bytes foram de fato carregados.
    reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
    let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;

    // 1. Handshake HELLO.
    let hello = read_packet(&reader)?;
    hello.parse_hello()?;
    send_packet(&writer, &Packet::hello())?;

    // 2. Metadados do arquivo.
    let meta_packet = read_packet(&reader)?;
    let meta = meta_packet.parse_meta()?;
    send_packet(&writer, &Packet::ack(0))?;

    // 3. Recebe o arquivo em chunks, confirmando cada um com ACK (ou
    // NACK em caso de corrupção/fora de ordem).
    let tmp_path = download_dir.join(format!("{}.part", sanitize_file_name(&meta.file_name)));
    let final_path = download_dir.join(sanitize_file_name(&meta.file_name));
    let mut file = std::fs::File::create(&tmp_path)?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut expected_index: u32 = 0;

    while received < meta.file_size {
        let packet = read_packet(&reader)?;
        match packet.packet_type {
            PacketType::Chunk => {
                let (index, data) = packet.parse_chunk()?;
                if index != expected_index {
                    send_packet(&writer, &Packet::nack(index, NackReason::OutOfOrder))?;
                    continue;
                }
                file.write_all(data)?;
                hasher.update(data);
                received += data.len() as u64;
                send_packet(&writer, &Packet::ack(index))?;
                expected_index += 1;
            }
            other => {
                return Err(ServerError::Protocol(protocol::ProtocolError::UnexpectedType {
                    expected: PacketType::Chunk,
                    actual: other,
                }))
            }
        }
    }
    file.flush()?;
    drop(file);

    // 4. Recalcula o SHA-256 do arquivo montado e confirma (ou não) a
    // integridade com um pacote DONE.
    let computed_hash: [u8; 32] = hasher.finalize().into();
    let integrity_ok = computed_hash == meta.sha256;
    send_packet(&writer, &Packet::done(integrity_ok))?;

    if !integrity_ok {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(ServerError::IntegrityCheckFailed);
    }

    std::fs::rename(&tmp_path, &final_path)?;
    Ok(meta.file_name)
}

/// Remove separadores de caminho e outros caracteres problemáticos do
/// nome de arquivo recebido, evitando escrita fora de `download_dir`.
fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

/// Lê um frame completo do socket: primeiro o cabeçalho fixo (para
/// saber o tamanho do payload), depois o payload + CRC.
fn read_packet(reader: &DataReader) -> Result<Packet, ServerError> {
    let header_bytes = read_exact(reader, HEADER_LEN as u32)?;
    let (_, payload_len) = Packet::decode_header(&header_bytes)?;

    let mut frame = header_bytes;
    let rest = read_exact(reader, payload_len + CRC_LEN as u32)?;
    frame.extend_from_slice(&rest);

    Ok(Packet::from_bytes(&frame)?)
}

/// Garante a leitura de exatamente `len` bytes do `DataReader`,
/// tratando o caso `InputStreamOptions::Partial` em que `LoadAsync`
/// pode retornar menos bytes do que o pedido.
fn read_exact(reader: &DataReader, len: u32) -> Result<Vec<u8>, ServerError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; len as usize];
    let mut filled = 0usize;
    while filled < buf.len() {
        let remaining = (buf.len() - filled) as u32;
        let loaded = reader.LoadAsync(remaining)?.get()?;
        if loaded == 0 {
            return Err(ServerError::Disconnected);
        }
        reader.ReadBytes(&mut buf[filled..filled + loaded as usize])?;
        filled += loaded as usize;
    }
    Ok(buf)
}

fn send_packet(writer: &DataWriter, packet: &Packet) -> Result<(), ServerError> {
    let bytes = packet.to_bytes();
    writer.WriteBytes(&bytes)?;
    writer.StoreAsync()?.get()?;
    Ok(())
}

