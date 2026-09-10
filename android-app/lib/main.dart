import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_bluetooth_serial/flutter_bluetooth_serial.dart';
import 'package:permission_handler/permission_handler.dart';

import 'file_transfer_service.dart';

void main() {
  runApp(const BtFileTransferApp());
}

class BtFileTransferApp extends StatelessWidget {
  const BtFileTransferApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'BT File Transfer',
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      home: const HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final FileTransferService _service = FileTransferService();

  List<BluetoothDevice> _bondedDevices = [];
  BluetoothDevice? _selectedDevice;
  File? _selectedFile;

  bool _isSending = false;
  double _progress = 0.0;
  String _status = 'Inicializando...';

  @override
  void initState() {
    super.initState();
    _init();
  }

  @override
  void dispose() {
    _service.disconnect();
    super.dispose();
  }

  Future<void> _init() async {
    final granted = await _requestBluetoothPermissions();
    if (!granted) {
      setState(() {
        _status = 'Permissoes de Bluetooth negadas. Conceda-as nas '
            'configuracoes do app para continuar.';
      });
      return;
    }
    await _loadBondedDevices();
  }

  /// BLUETOOTH_CONNECT e BLUETOOTH_SCAN são permissões "runtime" novas do
  /// Android 12 (API 31) que substituem o antigo `BLUETOOTH`/`BLUETOOTH_ADMIN`
  /// de tempo de instalação. Em versões mais antigas do Android o
  /// `permission_handler` resolve essas mesmas chaves para as permissões
  /// legadas automaticamente, então basta pedir as duas aqui.
  Future<bool> _requestBluetoothPermissions() async {
    final statuses = await [
      Permission.bluetoothConnect,
      Permission.bluetoothScan,
    ].request();
    return statuses.values.every((status) => status.isGranted);
  }

  Future<void> _loadBondedDevices() async {
    try {
      final devices = await FlutterBluetoothSerial.instance.getBondedDevices();
      setState(() {
        _bondedDevices = devices;
        _selectedDevice = devices.isNotEmpty ? devices.first : null;
        _status = devices.isEmpty
            ? 'Nenhum dispositivo pareado. Pareie o Windows pelas '
                'configuracoes de Bluetooth do Android antes de continuar.'
            : 'Selecione o dispositivo (Windows) e o arquivo a enviar.';
      });
    } catch (e) {
      setState(() => _status = 'Erro ao listar dispositivos pareados: $e');
    }
  }

  Future<void> _pickFile() async {
    final result = await FilePicker.platform.pickFiles();
    final path = result?.files.single.path;
    if (path == null) return;
    setState(() {
      _selectedFile = File(path);
      _status = 'Arquivo selecionado: ${result!.files.single.name}';
    });
  }

  Future<void> _sendFile() async {
    final device = _selectedDevice;
    final file = _selectedFile;
    if (device == null || file == null) return;

    setState(() {
      _isSending = true;
      _progress = 0.0;
      _status = 'Conectando a ${device.name ?? device.address}...';
    });

    try {
      await _service.connect(device);
      await _service.sendFile(
        file,
        onProgress: (progress) => setState(() => _progress = progress),
        onStatus: (status) => setState(() => _status = status),
      );
    } catch (e) {
      setState(() => _status = 'Erro na transferencia: $e');
    } finally {
      await _service.disconnect();
      setState(() => _isSending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final canSend = !_isSending && _selectedDevice != null && _selectedFile != null;

    return Scaffold(
      appBar: AppBar(title: const Text('BT File Transfer')),
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              DropdownButtonFormField<BluetoothDevice>(
                value: _selectedDevice,
                decoration: const InputDecoration(
                  labelText: 'Dispositivo pareado (Windows)',
                  border: OutlineInputBorder(),
                ),
                items: _bondedDevices
                    .map(
                      (device) => DropdownMenuItem(
                        value: device,
                        child: Text(device.name ?? device.address),
                      ),
                    )
                    .toList(),
                onChanged: _isSending ? null : (device) => setState(() => _selectedDevice = device),
              ),
              const SizedBox(height: 8),
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  onPressed: _isSending ? null : _loadBondedDevices,
                  icon: const Icon(Icons.refresh),
                  label: const Text('Atualizar dispositivos pareados'),
                ),
              ),
              const SizedBox(height: 16),
              OutlinedButton.icon(
                onPressed: _isSending ? null : _pickFile,
                icon: const Icon(Icons.attach_file),
                label: Text(
                  _selectedFile == null ? 'Escolher arquivo' : _selectedFile!.path.split('/').last,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(height: 24),
              FilledButton.icon(
                onPressed: canSend ? _sendFile : null,
                icon: const Icon(Icons.bluetooth_connected),
                label: const Text('Enviar via Bluetooth'),
              ),
              const SizedBox(height: 24),
              LinearProgressIndicator(value: _isSending ? _progress : 0),
              const SizedBox(height: 8),
              Text('${(_progress * 100).toStringAsFixed(0)}%', textAlign: TextAlign.center),
              const SizedBox(height: 16),
              Text(_status, textAlign: TextAlign.center),
            ],
          ),
        ),
      ),
    );
  }
}
