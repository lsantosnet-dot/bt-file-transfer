mod protocol;

#[cfg(target_os = "windows")]
mod server;

#[cfg(target_os = "windows")]
fn main() {
    if let Err(e) = server::run() {
        eprintln!("erro fatal ao iniciar o servidor: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!(
        "bt-file-transfer windows-server depende das APIs WinRT \
         (Windows.Devices.Bluetooth.*) e só roda em Windows 10/11.\n\
         Rodando em outra plataforma apenas o modulo `protocol` (framing \
         de pacotes) fica disponivel, e é o que os testes deste crate cobrem."
    );
}
